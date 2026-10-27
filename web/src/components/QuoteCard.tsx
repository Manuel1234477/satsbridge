'use client';

import { useEffect } from 'react';
import { AlertCircle, Info } from 'lucide-react';
import type { Quote } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
import { useCountdown } from '@/hooks/useCountdown';
import { useNow } from '@/hooks/useNow';

interface QuoteCardProps {
  quote: Quote;
  destinationExponent: number;
  sourceExponent: number;
  onExpired: () => void;
}

export function QuoteCard({ quote, sourceExponent, destinationExponent, onExpired }: QuoteCardProps) {
  const now = useNow();
  const { secondsLeft, isExpired } = useCountdown(quote.expires_at);
  const rateAgeSeconds = Math.max(0, Math.round((now - new Date(quote.rate_as_of).getTime()) / 1000));

  useEffect(() => {
    if (isExpired) onExpired();
  }, [isExpired, onExpired]);

  return (
    <section aria-labelledby="quote-heading" className="card card-content space-y-4">
      <h2 id="quote-heading" className="text-base font-semibold">
        Your quote
      </h2>

      <dl className="space-y-3 text-sm">
        <div className="flex justify-between gap-4">
          <dt className="text-muted">Rate</dt>
          <dd className="text-right font-medium tabular-nums">
            1 {quote.source_currency} ≈ {quote.exchange_rate} {quote.destination_currency}
          </dd>
        </div>
        <div className="flex justify-between gap-4">
          <dt className="text-muted">Fee</dt>
          <dd className="text-right font-medium tabular-nums">
            {formatWithCode(quote.fee_source_amount, sourceExponent, quote.source_currency)}
          </dd>
        </div>
        <div className="flex items-start justify-between gap-4 border-t border-[var(--card-border)] pt-3">
          <dt className="flex items-center gap-1 text-muted">
            Recipient receives
            {!quote.payout_amount_guaranteed && (
              <span className="group relative">
                <span className="rounded-full bg-warning-100 px-2 py-0.5 text-xs font-medium text-warning-800 dark:bg-warning-900 dark:text-warning-100">
                  Estimated
                </span>
                <span role="tooltip" className="invisible absolute left-0 top-full z-10 mt-1 w-56 rounded-md bg-neutral-900 p-2 text-xs text-white shadow group-hover:visible group-focus-within:visible">
                  The payout partner converts at its own rate.
                </span>
              </span>
            )}
          </dt>
          <dd className="text-right text-money-sm font-bold tabular-nums">
            {formatWithCode(quote.destination_amount, destinationExponent, quote.destination_currency)}
          </dd>
        </div>
      </dl>

      <p className="flex items-center gap-1 text-xs text-muted">
        <Info className="h-3.5 w-3.5" aria-hidden="true" />
        Live rate from {quote.rate_source} · {rateAgeSeconds}s ago
      </p>

      <div
        aria-live="polite"
        className={
          isExpired
            ? 'flex items-center gap-2 rounded-lg bg-warning-50 px-3 py-2 text-sm text-warning-800 dark:bg-warning-950 dark:text-warning-100'
            : secondsLeft <= 10
              ? 'flex items-center gap-2 rounded-lg bg-error-50 px-3 py-2 text-sm text-error-700 dark:bg-error-950 dark:text-error-200'
              : 'flex items-center gap-2 rounded-lg bg-neutral-100 px-3 py-2 text-sm text-neutral-700 dark:bg-neutral-800 dark:text-neutral-200'
        }
      >
        {isExpired ? <AlertCircle className="h-4 w-4" aria-hidden="true" /> : null}
        {isExpired ? 'Rate expired. Updating to the latest rate…' : `Quote expires in ${secondsLeft}s`}
      </div>
    </section>
  );
}
