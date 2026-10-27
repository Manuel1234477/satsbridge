'use client';

import { useCallback, useMemo, useState } from 'react';
import { ArrowDownUp } from 'lucide-react';
import { CurrencySelect } from '@/components/CurrencySelect';
import { MoneyField } from '@/components/MoneyField';
import { QuoteCard } from '@/components/QuoteCard';
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

  return (
    <div className="space-y-5">
      <h1 className="text-3xl font-bold">Send money</h1>

      <section className="card card-content space-y-4" aria-label="Amounts">
        <CurrencySelect
          label="You send"
          currencies={currencies}
          value={sourceCode}
          onlySendable
          onChange={(code) => {
            setSourceCode(code as Currency);
            setChosenRail('');
          }}
        />
        <MoneyField
          label="Amount you send"
          code={sourceCode}
          value={sourceDisplay}
          onChange={(text) => {
            setSide('source');
            setSourceText(text);
          }}
          disabled={!sourceInfo}
          error={side === 'source' && typedMinor === null && sourceText !== '' ? `Enter an amount with up to ${sourceExponent} decimals` : undefined}
        />

        <div className="flex justify-center">
          <button
            type="button"
            onClick={swap}
            disabled={!canSwap}
            aria-label="Swap currencies"
            className="btn btn-secondary p-2"
          >
            <ArrowDownUp className="h-5 w-5" aria-hidden="true" />
          </button>
        </div>

        <CurrencySelect
          label="They receive"
          currencies={currencies}
          value={destinationCode}
          onChange={(code) => {
            setDestinationCode(code as Currency);
            setChosenRail('');
          }}
        />
        {rails.length > 1 && (
          <div className="space-y-1">
            <label htmlFor="rail" className="block text-sm font-medium">
              Payout method
            </label>
            <select
              id="rail"
              value={activeRail ?? ''}
              onChange={(e) => setChosenRail(e.target.value as Rail)}
              className="form-input"
            >
              {rails.map((r) => (
                <option key={r.rail} value={r.rail}>
                  {RAIL_LABELS[r.rail]}
                </option>
              ))}
            </select>
          </div>
        )}
        <MoneyField
          label="Amount they receive"
          code={destinationCode}
          value={destinationDisplay}
          onChange={(text) => {
            setSide('destination');
            setDestinationText(text);
          }}
          disabled={!destinationInfo}
          hint={payoutHint}
          error={
            side === 'destination' && typedMinor === null && destinationText !== ''
              ? `Enter an amount with up to ${destinationExponent} decimals`
              : undefined
          }
          helper={
            quote && !quote.payout_amount_guaranteed ? (
              <p className="text-xs text-muted">Estimated: the payout partner converts at its own rate.</p>
            ) : null
          }
        />
      </section>

      <div aria-live="polite" className="space-y-3">
        {quoteQuery.isError && (
          <p role="alert" className="rounded-lg bg-error-50 px-3 py-2 text-sm text-error-700 dark:bg-error-950 dark:text-error-200">
            {quoteQuery.error.message}
          </p>
        )}
        {quote && (
          <QuoteCard
            quote={quote}
            sourceExponent={sourceExponent}
            destinationExponent={destinationExponent}
            onExpired={refreshQuote}
          />
        )}
        {quoteQuery.isFetching && !quote && <p className="text-sm text-muted">Getting your rate…</p>}
      </div>

      <button
        type="button"
        onClick={() => quote && activeRail && onContinue(quote, activeRail)}
        disabled={!continueEnabled || !activeRail}
        className="btn btn-primary w-full py-3 text-base"
      >
        Continue
      </button>
    </div>
  );
}
