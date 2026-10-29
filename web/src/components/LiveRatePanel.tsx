'use client';

import { useEffect } from 'react';
import { CheckCircle2, FileText, QrCode } from 'lucide-react';
import type { Quote } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
import { rateAgeSeconds } from '@/lib/rate-freshness';
import { useCountdown } from '@/hooks/useCountdown';
import { useNow } from '@/hooks/useNow';
import { StatusPill } from '@/components/ui/StatusPill';

interface LiveRatePanelProps {
  quote: Quote | undefined;
  sourceExponent: number;
  destinationExponent: number;
  stale: boolean;
  loading: boolean;
  onExpired: () => void;
  compact?: boolean;
}

function QuoteRing({ quote, size, secondsLeft }: { quote: Quote; size: number; secondsLeft: number }) {
  const total = Math.max(1, (new Date(quote.expires_at).getTime() - new Date(quote.created_at).getTime()) / 1000);
  const fraction = Math.min(1, Math.max(0, secondsLeft / total));
  const r = size / 2 - 4;
  const c = 2 * Math.PI * r;
  const low = secondsLeft <= 10;
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--raised)" strokeWidth="4" />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={low ? 'var(--pending)' : 'var(--accent)'}
        strokeWidth="4"
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={c * (1 - fraction)}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
        style={{ transition: 'stroke-dashoffset 900ms linear' }}
      />
      <text x="50%" y="50%" dominantBaseline="central" textAnchor="middle" fontSize={size / 4} fontWeight="600" fill="var(--text)" className="money">
        {secondsLeft}
      </text>
    </svg>
  );
}

export function LiveRatePanel({ quote, sourceExponent, destinationExponent, stale, loading, onExpired, compact }: LiveRatePanelProps) {
  const now = useNow();
  const { secondsLeft, isExpired } = useCountdown(quote?.expires_at);

  useEffect(() => {
    if (quote && isExpired) onExpired();
  }, [quote, isExpired, onExpired]);

  const body = !quote ? (
    <div className="flex flex-col gap-3" aria-busy={loading}>
      {loading ? (
        <>
          <div className="skeleton" style={{ height: compact ? 30 : 44, width: '75%' }} />
          <div className="skeleton" style={{ height: 16, width: '45%' }} />
        </>
      ) : (
        <p className="label">Enter an amount to see the live rate for this corridor.</p>
      )}
    </div>
  ) : (
    <>
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1.5">
          <p key={quote.exchange_rate} className={`value-in money ${compact ? 'text-[24px] font-semibold' : 'live-rate'}`}>
            1 {quote.source_currency} ≈ {quote.exchange_rate} {quote.destination_currency}
          </p>
          <p className="label">
            {quote.rate_source} · updated {rateAgeSeconds(quote.rate_as_of, now)}s ago
          </p>
        </div>
        <QuoteRing quote={quote} size={compact ? 48 : 64} secondsLeft={secondsLeft} />
      </div>

      <dl className="m-0">
        <div className="row-kv">
          <dt>Fee</dt>
          <dd className="money">{formatWithCode(quote.fee_source_amount, sourceExponent, quote.source_currency)}</dd>
        </div>
        <div className="row-kv">
          <dt>Recipient gets</dt>
          <dd className="money font-semibold">
            {formatWithCode(quote.destination_amount, destinationExponent, quote.destination_currency)}
            {!quote.payout_amount_guaranteed && <span className="status status-pending ml-2">Estimated</span>}
          </dd>
        </div>
      </dl>
      {!quote.payout_amount_guaranteed && <p className="label">Estimated: the payout partner converts at its own rate.</p>}

      <p aria-live="polite" className={isExpired ? 'text-sm text-[var(--pending)]' : 'label'}>
        {isExpired ? 'Quote expired. Updating…' : `Quote expires in ${secondsLeft}s`}
      </p>
    </>
  );

  return (
    <section aria-labelledby="live-rate-heading" className="card live-panel">
      <div className="flex items-center justify-between gap-2">
        <h2 id="live-rate-heading" className="label font-semibold uppercase tracking-wider" style={{ fontSize: 12 }}>
          Live rate
        </h2>
        {quote && (stale ? <StatusPill tone="pending" label="Stale" /> : <StatusPill tone="success" label="Live" />)}
      </div>
      {body}
      {!compact && (
        <div className="flex flex-col gap-3 border-t border-[var(--hairline)] pt-5">
          <h3 className="label font-semibold">How it works</h3>
          <ol className="steps-strip m-0 p-0">
            <li>
              <FileText className="h-5 w-5" aria-hidden="true" />
              <span className="font-semibold text-[var(--text)]">1. Quote</span>
              Lock a rate in your currency.
            </li>
            <li>
              <QrCode className="h-5 w-5" aria-hidden="true" />
              <span className="font-semibold text-[var(--text)]">2. Pay invoice</span>
              Pay once over Lightning.
            </li>
            <li>
              <CheckCircle2 className="h-5 w-5" aria-hidden="true" />
              <span className="font-semibold text-[var(--text)]">3. Recipient paid</span>
              They get their currency.
            </li>
          </ol>
        </div>
      )}
    </section>
  );
}
