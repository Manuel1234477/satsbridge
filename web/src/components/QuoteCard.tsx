'use client';

import { useEffect } from 'react';
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
  const totalSeconds = Math.max(1, (new Date(quote.expires_at).getTime() - new Date(quote.created_at).getTime()) / 1000);
  const fraction = Math.min(1, Math.max(0, secondsLeft / totalSeconds));

  useEffect(() => {
    if (isExpired) onExpired();
  }, [isExpired, onExpired]);

  return (
    <section aria-labelledby="quote-heading" className="card flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h2 id="quote-heading" className="text-base font-semibold">
          Your quote
        </h2>
        {!quote.payout_amount_guaranteed && <span className="status status-pending">Estimated</span>}
      </div>

      <dl className="m-0">
        <div className="row-kv">
          <dt>Rate</dt>
          <dd className="money">
            1 {quote.source_currency} ≈ {quote.exchange_rate} {quote.destination_currency}
          </dd>
        </div>
        <div className="row-kv">
          <dt>Fee</dt>
          <dd className="money">{formatWithCode(quote.fee_source_amount, sourceExponent, quote.source_currency)}</dd>
        </div>
        <div className="row-kv">
          <dt>They receive</dt>
          <dd className="money text-[length:var(--money-sm,22px)] font-semibold">
            {formatWithCode(quote.destination_amount, destinationExponent, quote.destination_currency)}
          </dd>
        </div>
      </dl>

      {!quote.payout_amount_guaranteed && (
        <p className="label">Estimated: the payout partner converts at its own rate.</p>
      )}

      <div aria-live="polite" className="flex flex-col gap-2">
        <div className="flex justify-between text-sm">
          <span className={isExpired ? 'text-[var(--pending)]' : 'label'}>
            {isExpired ? 'Rate expired. Updating…' : `Expires in ${secondsLeft}s`}
          </span>
          <span className="label">
            {quote.rate_source} · {rateAgeSeconds}s ago
          </span>
        </div>
        <div className="progress-track" role="presentation">
          <div className={`progress-fill${secondsLeft <= 10 ? ' is-low' : ''}`} style={{ width: `${fraction * 100}%` }} />
        </div>
      </div>
    </section>
  );
}
