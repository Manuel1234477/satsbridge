'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { getTransactions } from '@/lib/api';
import type { CurrencyInfo, PaymentIntent, PaymentStatus } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
import { useCurrencies } from '@/hooks/useCurrencies';
import { StatusPill } from '@/components/ui/StatusPill';
import { Skeleton } from '@/components/ui/Skeleton';

type Tone = 'success' | 'pending' | 'failed' | 'refunded';

const STATUS: Record<PaymentStatus, { tone: Tone; label: string }> = {
  CREATED: { tone: 'pending', label: 'Created' },
  QUOTED: { tone: 'pending', label: 'Created' },
  AWAITING_PAYMENT: { tone: 'pending', label: 'Awaiting payment' },
  PAYMENT_DETECTED: { tone: 'pending', label: 'In progress' },
  SETTLED: { tone: 'pending', label: 'In progress' },
  PAYOUT_PENDING: { tone: 'pending', label: 'In progress' },
  COMPLETED: { tone: 'success', label: 'Delivered' },
  EXPIRED: { tone: 'failed', label: 'Expired' },
  FAILED: { tone: 'failed', label: 'Failed' },
  REFUND_PENDING: { tone: 'pending', label: 'Refund needed' },
  REFUNDED: { tone: 'refunded', label: 'Refunded' },
};

function expOf(currencies: CurrencyInfo[], code: string): number {
  return currencies.find((c) => c.code === code)?.exponent ?? 0;
}

function dayLabel(iso: string, now: Date): string {
  const date = new Date(iso);
  const startOf = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diffDays = Math.round((startOf(now) - startOf(date)) / 86_400_000);
  if (diffDays === 0) return 'Today';
  if (diffDays === 1) return 'Yesterday';
  return date.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}

function groupByDay(list: PaymentIntent[]): { label: string; items: PaymentIntent[] }[] {
  const now = new Date();
  const groups: { label: string; items: PaymentIntent[] }[] = [];
  for (const item of list) {
    const label = dayLabel(item.created_at, now);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.items.push(item);
    else groups.push({ label, items: [item] });
  }
  return groups;
}

export function HistoryList() {
  const currencies = useCurrencies();
  const transactions = useQuery({ queryKey: ['transactions'], queryFn: () => getTransactions(50), retry: false });

  if (transactions.isPending || currencies.isPending) {
    return (
      <div className="page" aria-busy="true" aria-label="Loading activity">
        <Skeleton height={76} />
        <Skeleton height={76} />
        <Skeleton height={76} />
      </div>
    );
  }
  if (transactions.isError || currencies.isError) {
    const message = (transactions.error ?? currencies.error)?.message ?? 'Something went wrong.';
    return (
      <div className="page">
        <p role="alert" className="notice notice-failed">
          {message}
        </p>
        <button type="button" onClick={() => transactions.refetch()} className="btn btn-quiet btn-block">
          Try again
        </button>
      </div>
    );
  }

  const list = transactions.data;
  if (list.length === 0) {
    return (
      <div className="page items-center text-center">
        <h1 className="page-title">No activity yet</h1>
        <p className="label">Payments you make will appear here.</p>
        <Link href="/" className="btn btn-accent">
          Send money
        </Link>
      </div>
    );
  }

  const groups = groupByDay(list);
  return (
    <div className="page">
      <h1 className="page-title">Activity</h1>
      {groups.map((group) => (
        <section key={group.label} aria-label={group.label} className="flex flex-col gap-2">
          <h2 className="label">{group.label}</h2>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {group.items.map((p) => {
              const status = STATUS[p.status];
              return (
                <li key={p.id}>
                  <Link href={`/payments/${p.id}`} className="list-row">
                    <span className="badge-stack" aria-hidden="true">
                      <span className="badge">{p.source_currency.slice(0, 2)}</span>
                      <span className="badge">{p.destination_currency.slice(0, 2)}</span>
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-semibold">{p.recipient_name ?? p.recipient_masked}</span>
                      <span className="label">
                        {p.source_currency} → {p.destination_currency} · {new Date(p.created_at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}
                      </span>
                    </span>
                    <span className="flex flex-col items-end gap-1">
                      <span className="money font-semibold">
                        {formatWithCode(p.source_amount, expOf(currencies.data, p.source_currency), p.source_currency)}
                      </span>
                      <StatusPill tone={status.tone} label={status.label} />
                    </span>
                  </Link>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
