'use client';

import { useCallback, useMemo, useRef, useState } from 'react';
import { ApiError } from '@/lib/api/errors';
import { isRateStale } from '@/lib/rate-freshness';
import { useNow } from '@/hooks/useNow';
import Link from 'next/link';
import { ArrowDownUp } from 'lucide-react';
import { QuoteCard } from '@/components/QuoteCard';
import { AmountInput } from '@/components/ui/AmountInput';
import { CurrencyChip } from '@/components/ui/CurrencyChip';
import { CurrencySheet } from '@/components/ui/CurrencySheet';
import { Sheet } from '@/components/ui/Sheet';
import { Skeleton } from '@/components/ui/Skeleton';
import { useCountdown } from '@/hooks/useCountdown';
import { useDebounced } from '@/hooks/useDebounced';
import { useQuotePreview } from '@/hooks/useQuotePreview';
import type { CreateQuoteInput } from '@/lib/api';
import type { CurrencyInfo, Currency, Quote, Rail } from '@/lib/api/schemas';
import { parseMinor, toInputText } from '@/lib/money';
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
  const [sheet, setSheet] = useState<'source' | 'destination' | 'rail' | 'help' | null>(null);
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

  const sourceDisplay = side === 'source' ? sourceText : quote ? toInputText(quote.source_amount, sourceExponent) : '';
  const destinationDisplay =
    side === 'destination' ? destinationText : quote ? toInputText(quote.destination_amount, destinationExponent) : '';

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
        return `Payouts via ${RAIL_LABELS[activeRail]}: ${toInputText(rail.min_payout_minor, destinationExponent)} to ${toInputText(rail.max_payout_minor, destinationExponent)} ${destinationCode}`;
      })()
    : undefined;

  const continueEnabled = quote !== undefined && !pending && !isExpired && quoteQuery.isSuccess;
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
              : 'Fix the amount to continue'
            : '';

  return (
    <div className="page">
      <h1 className="sr-only">Send money</h1>
      <section aria-label="You send" className="flex flex-col items-center gap-3 pt-6">
        <p className="label">You send</p>
        <AmountInput
          label="Amount you send"
          variant="hero"
          inputRef={amountRef}
          value={sourceDisplay}
          disabled={!sourceInfo}
          onChange={(text) => {
            setSide('source');
            setSourceText(text);
          }}
        />
        <CurrencyChip code={sourceCode} onClick={() => setSheet('source')} />
        {side === 'source' && typedMinor === null && sourceText !== '' && (
          <p role="alert" className="error-text">
            Enter an amount with up to {sourceExponent} decimals
          </p>
        )}
      </section>

      <div className="flex justify-center">
        <button type="button" onClick={swap} disabled={!canSwap} aria-label="Swap currencies" className="round-action">
          <span className="disc">
            <ArrowDownUp className="h-5 w-5" aria-hidden="true" />
          </span>
        </button>
      </div>

      <section className="card flex flex-col gap-3" aria-label="They receive">
        <div className="flex items-center justify-between gap-3">
          <p className="label">They receive</p>
          <CurrencyChip code={destinationCode} onClick={() => setSheet('destination')} />
        </div>
        <AmountInput
          label="Amount they receive"
          variant="card"
          value={destinationDisplay}
          disabled={!destinationInfo}
          onChange={(text) => {
            setSide('destination');
            setDestinationText(text);
          }}
        />
        {payoutHint && <p className="label">{payoutHint}</p>}
        {side === 'destination' && typedMinor === null && destinationText !== '' && (
          <p role="alert" className="error-text">
            Enter an amount with up to {destinationExponent} decimals
          </p>
        )}
      </section>

      <nav aria-label="Actions" className="flex justify-between px-2">
        <button type="button" className="round-action" onClick={() => amountRef.current?.focus()}>
          <span className="disc" aria-hidden="true">
            ↑
          </span>
          Send
        </button>
        <button type="button" className="round-action" disabled={rails.length < 2} onClick={() => setSheet('rail')}>
          <span className="disc" aria-hidden="true">
            ⇄
          </span>
          Rail
        </button>
        <Link href="/history" className="round-action" style={{ textDecoration: 'none' }}>
          <span className="disc" aria-hidden="true">
            ≡
          </span>
          Activity
        </Link>
        <button type="button" className="round-action" onClick={() => setSheet('help')}>
          <span className="disc" aria-hidden="true">
            ?
          </span>
          Help
        </button>
      </nav>

      <div aria-live="polite" className="flex flex-col gap-3">
        {quoteQuery.isError && (
          <div className="notice notice-failed flex-col">
            <p role="alert">{quoteQuery.error.message}</p>
            {retryable && (
              <button type="button" className="btn btn-quiet btn-block" onClick={() => void refetchQuote()} disabled={quoteQuery.isFetching}>
                {quoteQuery.isFetching ? 'Trying again…' : 'Try again'}
              </button>
            )}
          </div>
        )}
        {quote && (
          <QuoteCard
            quote={quote}
            sourceExponent={sourceExponent}
            destinationExponent={destinationExponent}
            stale={isRateStale(quote.rate_as_of, now, quoteQuery.isError)}
            onExpired={refreshQuote}
          />
        )}
        {quoteQuery.isFetching && !quote && <Skeleton height={120} />}
      </div>

      <div className="dock">
        {reason && <p className="label text-center">{reason}</p>}
        <button
          type="button"
          onClick={() => quote && activeRail && onContinue(quote, activeRail)}
          disabled={!continueEnabled || !activeRail}
          className="btn btn-accent btn-block"
        >
          Continue
        </button>
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
      <Sheet open={sheet === 'rail'} onClose={() => setSheet(null)} title="Payout method">
        <div className="sheet-list" role="listbox" aria-label="Payout method">
          {rails.map((r) => (
            <button
              key={r.rail}
              type="button"
              role="option"
              aria-selected={r.rail === activeRail}
              className="sheet-option"
              onClick={() => {
                setChosenRail(r.rail);
                setSheet(null);
              }}
            >
              <span className="flex-1">
                <span className="block font-semibold">{RAIL_LABELS[r.rail]}</span>
                <span className="block text-sm text-[var(--text-2)]">
                  {r.guaranteed_amount ? 'Guaranteed amount' : 'Estimated: the provider converts at its own rate'}
                </span>
              </span>
            </button>
          ))}
        </div>
      </Sheet>
      <Sheet open={sheet === 'help'} onClose={() => setSheet(null)} title="How this works">
        <p className="text-[var(--text-2)]">
          Enter an amount, check the rate, then confirm the recipient. You pay a Lightning invoice, and we pay the recipient in their currency.
        </p>
        <p className="text-[var(--text-2)]">Demo mode: no real money is moved.</p>
      </Sheet>
    </div>
  );
}
