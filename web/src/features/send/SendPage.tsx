'use client';

import { useCallback, useMemo, useRef, useState } from 'react';
import { ApiError } from '@/lib/api/errors';
import { isRateStale } from '@/lib/rate-freshness';
import { useNow } from '@/hooks/useNow';
import Link from 'next/link';
import { ArrowDownUp, CircleHelp, History, Send } from 'lucide-react';
import { LiveRatePanel } from '@/components/LiveRatePanel';
import { InlineAlert } from '@/components/ui/InlineAlert';
import { RailSelector } from '@/components/ui/RailSelector';
import { useAnimatedNumber } from '@/hooks/useAnimatedNumber';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import { AmountInput } from '@/components/ui/AmountInput';
import { CurrencyChip } from '@/components/ui/CurrencyChip';
import { CurrencySheet } from '@/components/ui/CurrencySheet';
import { Sheet } from '@/components/ui/Sheet';
import { useCountdown } from '@/hooks/useCountdown';
import { useDebounced } from '@/hooks/useDebounced';
import { useQuotePreview } from '@/hooks/useQuotePreview';
import type { CreateQuoteInput } from '@/lib/api';
import type { CurrencyInfo, Currency, Quote, Rail } from '@/lib/api/schemas';
import { formatMinor, parseMinor, toInputText } from '@/lib/money';
import { RAIL_LABELS } from '@/lib/rails';

export interface SendPrefill {
  sourceCurrency: Currency;
  destinationCurrency: Currency;
  sourceMinor: number;
}

interface SendPageProps {
  currencies: CurrencyInfo[];
  prefill?: SendPrefill;
  onContinue: (quote: Quote, rail: Rail) => void;
}

const DEFAULT_SOURCE: Currency = 'XOF';
const DEFAULT_DESTINATION: Currency = 'NGN';

export function SendPage({ currencies, prefill, onContinue }: SendPageProps) {
  const [sourceCode, setSourceCode] = useState<Currency>(prefill?.sourceCurrency ?? DEFAULT_SOURCE);
  const [destinationCode, setDestinationCode] = useState<Currency>(prefill?.destinationCurrency ?? DEFAULT_DESTINATION);
  const [side, setSide] = useState<'source' | 'destination'>('source');
  const [sourceText, setSourceText] = useState(() => {
    const exponent = currencies.find((c) => c.code === (prefill?.sourceCurrency ?? DEFAULT_SOURCE))?.exponent ?? 0;
    return prefill ? toInputText(prefill.sourceMinor, exponent) : '';
  });
  const [destinationText, setDestinationText] = useState('');
  const [chosenRail, setChosenRail] = useState<Rail | ''>('');
  const [sheet, setSheet] = useState<'source' | 'destination' | 'help' | null>(null);
  const amountRef = useRef<HTMLInputElement>(null);

  const sourceInfo = currencies.find((c) => c.code === sourceCode);
  const destinationInfo = currencies.find((c) => c.code === destinationCode);
  const sourceExponent = sourceInfo?.exponent ?? 0;
  const destinationExponent = destinationInfo?.exponent ?? 0;
  const rails = destinationInfo?.payout_rails ?? [];
  const activeRail: Rail | null = rails.find((r) => r.rail === chosenRail)?.rail ?? rails[0]?.rail ?? null;

  const typedMinor =
    side === 'source' ? parseMinor(sourceText, sourceExponent) : parseMinor(destinationText, destinationExponent);

  const inputKey =
    sourceInfo && destinationInfo && activeRail && sourceInfo.can_send && typedMinor !== null && typedMinor > 0
      ? JSON.stringify({
          source_currency: sourceCode,
          destination_currency: destinationCode,
          amount: typedMinor,
          amount_side: side,
          rail: activeRail,
        })
      : '';
  const debouncedKey = useDebounced(inputKey, 400);
  const debouncedInput = useMemo<CreateQuoteInput | null>(
    () => (debouncedKey ? (JSON.parse(debouncedKey) as CreateQuoteInput) : null),
    [debouncedKey],
  );
  const quoteQuery = useQuotePreview(debouncedInput);
  const quote = quoteQuery.data;
  const pending = inputKey !== debouncedKey || quoteQuery.isFetching;

  const { isExpired } = useCountdown(quote?.expires_at);
  const now = useNow();
  const retryable = quoteQuery.error instanceof ApiError && (quoteQuery.error.code === 'provider_unavailable' || quoteQuery.error.code === 'rate_limited');
  const refetchQuote = quoteQuery.refetch;
  const refreshQuote = useCallback(() => {
    void refetchQuote();
  }, [refetchQuote]);


  const canSwap = destinationInfo?.can_send === true;
  const swap = () => {
    if (!canSwap) return;
    setSourceCode(destinationCode);
    setDestinationCode(sourceCode);
    setSourceText(side === 'destination' ? destinationText : '');
    setDestinationText(side === 'source' ? sourceText : '');
    setSide(side === 'source' ? 'destination' : 'source');
    setChosenRail('');
  };

  const payoutHint = destinationInfo && activeRail
    ? (() => {
        const rail = rails.find((r) => r.rail === activeRail);
        if (!rail || rail.min_payout_minor === null || rail.max_payout_minor === null) return undefined;
        return `Payouts via ${RAIL_LABELS[activeRail]}: ${formatMinor(rail.min_payout_minor, destinationExponent)} to ${formatMinor(rail.max_payout_minor, destinationExponent)} ${destinationCode}`;
      })()
    : undefined;

  const continueEnabled = quote !== undefined && !pending && !isExpired && quoteQuery.isSuccess;
  const isDesktop = useMediaQuery('(min-width: 1024px)');
  const animatedDestination = useAnimatedNumber(quote?.destination_amount);
  const animatedSource = useAnimatedNumber(quote?.source_amount);
  const sourceShown =
    side === 'source' ? sourceText : animatedSource !== undefined ? toInputText(animatedSource, sourceExponent) : '';
  const destinationShown =
    side === 'destination'
      ? destinationText
      : animatedDestination !== undefined
        ? toInputText(animatedDestination, destinationExponent)
        : '';

  const quoteError = quoteQuery.error;
  const validationProblem = quoteError instanceof ApiError && quoteError.code === 'validation_error';
  const canRetry = quoteError !== null && !validationProblem;
  const alertMessage = validationProblem
    ? payoutHint
      ? `Check the amount. ${payoutHint}.`
      : 'Check the amount and try a different value.'
    : (quoteError?.message ?? '');

  const reason = !activeRail
    ? 'Choose a payout method'
    : typedMinor === null
      ? 'Enter an amount to continue'
      : isExpired
        ? 'Rate expired. Getting a new one…'
        : pending || quoteQuery.isFetching
          ? 'Getting your rate…'
          : quoteQuery.isError
            ? retryable
              ? 'Could not get a rate. Try again.'
              : 'Check the amount to continue'
            : '';

  const panel = (
    <LiveRatePanel
      quote={quote}
      sourceExponent={sourceExponent}
      destinationExponent={destinationExponent}
      stale={quote ? isRateStale(quote.rate_as_of, now, quoteQuery.isError) : false}
      loading={quoteQuery.isFetching || (inputKey !== '' && inputKey !== debouncedKey)}
      onExpired={refreshQuote}
      compact={!isDesktop}
    />
  );

  return (
    <div className="page">
      <h1 className="sr-only">Send money</h1>
      <div className="send-grid">
        <div className="flex flex-col gap-5">
          <section aria-label="Amounts" className="card flex flex-col gap-5">
            <div className="flex flex-col items-center gap-3 pt-2">
              <p className="label">You send</p>
              <AmountInput
                label="Amount you send"
                variant="hero"
                inputRef={amountRef}
                value={sourceShown}
                disabled={!sourceInfo}
                onChange={(text) => {
                  setSide('source');
                  setSourceText(text);
                }}
              />
              <CurrencyChip code={sourceCode} onClick={() => setSheet('source')} />
              {side === 'source' && typedMinor === null && sourceText !== '' && (
                <p className="label">Enter an amount with up to {sourceExponent} decimals.</p>
              )}
            </div>

            <div className="relative flex items-center justify-center">
              <span className="absolute inset-x-0 top-1/2 h-px bg-[var(--hairline)]" aria-hidden="true" />
              <button type="button" onClick={swap} disabled={!canSwap} aria-label="Swap currencies" className="round-action relative">
                <span className="disc">
                  <ArrowDownUp className="h-5 w-5" aria-hidden="true" />
                </span>
              </button>
            </div>

            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between gap-3">
                <p className="label">They receive</p>
                <CurrencyChip code={destinationCode} onClick={() => setSheet('destination')} />
              </div>
              <AmountInput
                label="Amount they receive"
                variant="card"
                value={destinationShown}
                disabled={!destinationInfo}
                onChange={(text) => {
                  setSide('destination');
                  setDestinationText(text);
                }}
              />
              {rails.length > 1 && (
                <RailSelector rails={rails.map((r) => r.rail)} value={activeRail} onChange={(rail) => setChosenRail(rail)} />
              )}
              {payoutHint && <p className="label">{payoutHint}</p>}
              {side === 'destination' && typedMinor === null && destinationText !== '' && (
                <p className="label">Enter an amount with up to {destinationExponent} decimals.</p>
              )}
            </div>
          </section>

          {quoteQuery.isError && (
            <InlineAlert
              message={alertMessage}
              onRetry={canRetry ? () => void refetchQuote() : undefined}
              retrying={quoteQuery.isFetching}
            />
          )}

          {!isDesktop && panel}

          <nav aria-label="Shortcuts" className="flex justify-around">
            <button type="button" className="round-action" onClick={() => amountRef.current?.focus()}>
              <span className="disc">
                <Send className="h-5 w-5" aria-hidden="true" />
              </span>
              Send
            </button>
            <Link href="/history" className="round-action">
              <span className="disc">
                <History className="h-5 w-5" aria-hidden="true" />
              </span>
              Activity
            </Link>
            <button type="button" className="round-action" onClick={() => setSheet('help')}>
              <span className="disc">
                <CircleHelp className="h-5 w-5" aria-hidden="true" />
              </span>
              Help
            </button>
          </nav>

          <div className="dock">
            {reason && (
              <p className="label text-center" aria-live="polite">
                {reason}
              </p>
            )}
            <button
              type="button"
              onClick={() => quote && activeRail && onContinue(quote, activeRail)}
              disabled={!continueEnabled || !activeRail}
              className="btn btn-accent btn-block"
            >
              Continue
            </button>
          </div>
        </div>

        {isDesktop && <aside className="live-panel-sticky">{panel}</aside>}
      </div>

      <CurrencySheet
        open={sheet === 'source'}
        onClose={() => setSheet(null)}
        title="Send currency"
        currencies={currencies}
        selected={sourceCode}
        onlySendable
        onSelect={(code) => {
          setSourceCode(code as Currency);
          setChosenRail('');
        }}
      />
      <CurrencySheet
        open={sheet === 'destination'}
        onClose={() => setSheet(null)}
        title="Receive currency"
        currencies={currencies}
        selected={destinationCode}
        onSelect={(code) => {
          setDestinationCode(code as Currency);
          setChosenRail('');
        }}
      />
      <Sheet open={sheet === 'help'} onClose={() => setSheet(null)} title="How this works">
        <p className="label">
          Enter an amount and check the live rate. Confirm the recipient, then pay one Lightning invoice. The recipient is paid in their own currency.
        </p>
        <p className="label">Sandbox: no real money is moved.</p>
      </Sheet>
    </div>
  );
}
