'use client';

import { useEffect } from 'react';
import type { Quote } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
import { rateAgeSeconds } from '@/lib/rate-freshness';
import { useCountdown } from '@/hooks/useCountdown';
import { useNow } from '@/hooks/useNow';
import { StatusPill } from '@/components/ui/StatusPill';

interface QuoteCardProps {
  quote: Quote;
  destinationExponent: number;
  sourceExponent: number;
  stale: boolean;
  onExpired: () => void;
}

export function QuoteCard({ quote, sourceExponent, destinationExponent, stale, onExpired }: QuoteCardProps) {
  const now = useNow();
  const { secondsLeft, isExpired } = useCountdown(quote.expires_at);
  const totalSeconds = Math.max(1, (new Date(quote.expires_at).getTime() - new Date(quote.created_at).getTime()) / 1000);
  const fraction = Math.min(1, Math.max(0, secondsLeft / totalSeconds));

  useEffect(() => {
    if (isExpired) onExpired();
  }, [isExpired, onExpired]);

  return (
    <section aria-labelledby="quote-heading" className="card flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h2 id="quote-heading" className="label">
          Live rate
        </h2>
        {stale ? <StatusPill tone="pending" label="Stale" /> : <StatusPill tone="success" label="Live" />}
      </div>

      <p className="money text-[26px] font-semibold">
        1 {quote.source_currency} ≈ {quote.exchange_rate} {quote.destination_currency}
      </p>
      <p className="label">
        {quote.rate_source} · updated {rateAgeSeconds(quote.rate_as_of, now)}s ago
      </p>

      <dl className="m-0">
        <div className="row-kv">
          <dt>Fee</dt>
          <dd className="money">{formatWithCode(quote.fee_source_amount, sourceExponent, quote.source_currency)}</dd>
        </div>
        <div className="row-kv">
          <dt>They receive</dt>
          <dd className="money font-semibold">
            {formatWithCode(quote.destination_amount, destinationExponent, quote.destination_currency)}
            {!quote.payout_amount_guaranteed && <span className="status status-pending ml-2">Estimated</span>}
          </dd>
        </div>
      </dl>

      {!quote.payout_amount_guaranteed && <p className="label">Estimated: the payout partner converts at its own rate.</p>}

      <div aria-live="polite" className="flex flex-col gap-2">
        <div className="flex justify-between text-sm">
          <span className={isExpired ? 'text-[var(--pending)]' : 'label'}>
            {isExpired ? 'Quote expired. Updating…' : `Quote expires in ${secondsLeft}s`}
          </span>
        </div>
        <div className="progress-track" role="presentation">
          <div className={`progress-fill${secondsLeft <= 10 ? ' is-low' : ''}`} style={{ width: `${fraction * 100}%` }} />
        </div>
      </div>
    </section>
  );
}
