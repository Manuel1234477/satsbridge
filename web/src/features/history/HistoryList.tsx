'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { getTransactions } from '@/lib/api';
import type { CurrencyInfo, PaymentIntent } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
import { useCurrencies } from '@/hooks/useCurrencies';

const STATUS_CLASS: Record<string, string> = {
  COMPLETED: 'bg-success-100 text-success-900 dark:bg-success-900 dark:text-success-100',
  FAILED: 'bg-error-100 text-error-900 dark:bg-error-900 dark:text-error-100',
  EXPIRED: 'bg-error-100 text-error-900 dark:bg-error-900 dark:text-error-100',
  REFUND_PENDING: 'bg-warning-100 text-warning-900 dark:bg-warning-900 dark:text-warning-100',
  REFUNDED: 'bg-neutral-200 text-neutral-900 dark:bg-neutral-700 dark:text-neutral-100',
};

function expOf(currencies: CurrencyInfo[], code: string): number {
  return currencies.find((c) => c.code === code)?.exponent ?? 0;
}

export function HistoryList() {
  const currencies = useCurrencies();
  const transactions = useQuery({ queryKey: ['transactions'], queryFn: () => getTransactions(50), retry: false });

  if (transactions.isPending || currencies.isPending) return <p className="text-muted">Loading your payments…</p>;
  if (transactions.isError) {
    return (
      <div className="space-y-3">
        <p role="alert" className="text-error-700 dark:text-error-300">{transactions.error.message}</p>
        <button type="button" onClick={() => transactions.refetch()} className="btn btn-secondary">
          Try again
        </button>
      </div>
    );
  }
  if (currencies.isError) return <p role="alert" className="text-error-700 dark:text-error-300">{currencies.error.message}</p>;

  const list: PaymentIntent[] = transactions.data;
  if (list.length === 0) {
    return (
      <div className="space-y-4 text-center">
        <h1 className="text-2xl font-bold">No payments yet</h1>
        <p className="text-muted">Payments you make will appear here.</p>
        <Link href="/" className="btn btn-primary">
          Send money
        </Link>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-bold">Payments</h1>
      <ul className="space-y-2">
        {list.map((p) => (
          <li key={p.id}>
            <Link href={`/payments/${p.id}`} className="card flex items-center justify-between gap-4 p-4 hover:bg-neutral-50 dark:hover:bg-neutral-800">
              <div>
                <p className="font-medium">
                  {p.source_currency} → {p.destination_currency}
                </p>
                <p className="text-xs text-muted">{new Date(p.created_at).toLocaleString()}</p>
              </div>
              <div className="text-right">
                <p className="font-semibold tabular-nums">
                  {formatWithCode(p.source_amount, expOf(currencies.data, p.source_currency), p.source_currency)}
                </p>
                <span className={`mt-1 inline-block rounded-full px-2 py-0.5 text-xs font-medium ${STATUS_CLASS[p.status] ?? 'bg-neutral-100 text-neutral-800 dark:bg-neutral-800 dark:text-neutral-100'}`}>
                  {p.status.replace(/_/g, ' ').toLowerCase()}
                </span>
              </div>
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}
