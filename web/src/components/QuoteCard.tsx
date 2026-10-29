'use client';

import { Quote, CurrencyInfo } from '@/lib/api/schemas';
import { formatMoney, formatMoneyWithCurrency } from '@/lib/money';
import { useCountdown } from '@/hooks/useCountdown';
import { AlertCircle } from 'lucide-react';

interface QuoteCardProps {
  quote: Quote;
  currencies: CurrencyInfo[];
  isLoading?: boolean;
  onRefresh?: () => void;
}

export function QuoteCard({ quote, currencies, isLoading = false, onRefresh }: QuoteCardProps) {
  const { secondsLeft, isExpired } = useCountdown(quote.expires_at);

  const getCurrencyInfo = (code: string) => currencies.find(c => c.code === code);
  const sourceCurr = getCurrencyInfo(quote.source_currency);
  const destCurr = getCurrencyInfo(quote.destination_currency);

  const rateAsOfSeconds = Math.floor((new Date().getTime() - new Date(quote.rate_as_of).getTime()) / 1000);

  return (
    <div className="card animate-fade-in">
      <div className="card-content space-y-4">
        {/* Header */}
        <div className="flex items-start justify-between">
          <h3 className="text-lg font-semibold">Exchange rate</h3>
          {isExpired && (
            <button
              onClick={onRefresh}
              disabled={isLoading}
              className="btn btn-ghost text-sm"
            >
              {isLoading ? 'Refreshing...' : 'Refresh'}
            </button>
          )}
        </div>

        {/* Status message */}
        {isExpired && (
          <div className="flex gap-2 p-3 rounded-lg bg-warning-50 dark:bg-warning-950 text-warning-700 dark:text-warning-300">
            <AlertCircle className="w-5 h-5 flex-shrink-0 mt-0.5" />
            <p className="text-sm">Rate expired. Refreshing to get the latest rate.</p>
          </div>
        )}

        {/* Exchange rate */}
        <div className="space-y-1">
          <p className="text-sm text-muted">
            1 {quote.source_currency} ≈{' '}
            <span className="font-semibold text-foreground">{quote.exchange_rate}</span> {quote.destination_currency}
          </p>
          <p className="text-xs text-muted">
            {quote.rate_source} · {rateAsOfSeconds}s ago
          </p>
        </div>

        {/* Fee */}
        <div className="flex justify-between">
          <span className="text-sm text-muted">Fee:</span>
          <span className="font-medium">
            {formatMoneyWithCurrency(quote.fee_source_amount, quote.source_currency)}
          </span>
        </div>

        {/* Recipient receives */}
        <div className="flex justify-between items-baseline gap-4">
          <span className="text-sm text-muted">
            Recipient receives:
            {!quote.payout_amount_guaranteed && (
              <span className="ml-1 text-xs">
                (Estimated - provider's rate)
              </span>
            )}
          </span>
          <span className="text-money-sm font-bold">
            {formatMoneyWithCurrency(quote.destination_amount, quote.destination_currency)}
          </span>
        </div>

        {/* Countdown */}
        <div className={`flex items-center justify-between px-3 py-2 rounded-lg ${
          secondsLeft <= 10
            ? 'bg-error-50 dark:bg-error-950 text-error-700 dark:text-error-300'
            : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300'
        }`}>
          <span className="text-sm font-medium">
            {isExpired ? 'Expired' : `Expires in ${secondsLeft}s`}
          </span>
          <div className={`w-2 h-2 rounded-full ${
            isExpired ? 'bg-error-600' : secondsLeft <= 10 ? 'bg-warning-600' : 'bg-success-600'
          } animate-pulse`} />
        </div>
      </div>
    </div>
  );
}
