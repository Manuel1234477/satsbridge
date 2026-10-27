'use client';

import { useQuery } from '@tanstack/react-query';
import { getTransactions } from '@/lib/api';
import { formatMoneyWithCurrency } from '@/lib/money';
import { useRouter } from 'next/navigation';
import { AlertCircle } from 'lucide-react';

export default function HistoryPage() {
  const router = useRouter();
  const { data: transactions, isLoading, error } = useQuery({
    queryKey: ['transactions'],
    queryFn: () => getTransactions(50),
  });

  const statusBadgeColor = (status: string) => {
    switch (status) {
      case 'COMPLETED':
        return 'bg-success-100 dark:bg-success-900 text-success-900 dark:text-success-100';
      case 'FAILED':
      case 'EXPIRED':
        return 'bg-error-100 dark:bg-error-900 text-error-900 dark:text-error-100';
      case 'PAYOUT_PENDING':
        return 'bg-warning-100 dark:bg-warning-900 text-warning-900 dark:text-warning-100';
      default:
        return 'bg-neutral-100 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100';
    }
  };

  return (
    <main className="min-h-screen bg-white dark:bg-neutral-950 py-8">
      <div className="container-safe max-w-2xl">
        <h1 className="text-3xl font-bold mb-8">Payment history</h1>

        {isLoading && (
          <div className="text-center py-12 text-muted">
            Loading transactions...
          </div>
        )}

        {error && (
          <div className="flex gap-3 p-4 rounded-lg bg-error-50 dark:bg-error-950 border border-error-200 dark:border-error-800">
            <AlertCircle className="w-5 h-5 text-error-600 dark:text-error-400 flex-shrink-0 mt-0.5" />
            <div>
              <p className="font-medium text-error-900 dark:text-error-100">Failed to load history</p>
              <p className="text-sm text-error-700 dark:text-error-300">{error?.message}</p>
            </div>
          </div>
        )}

        {transactions && transactions.transactions.length === 0 && (
          <div className="text-center py-12">
            <p className="text-lg text-muted mb-4">No transactions yet</p>
            <button
              onClick={() => router.push('/')}
              className="btn btn-primary"
            >
              Send your first payment
            </button>
          </div>
        )}

        {transactions && transactions.transactions.length > 0 && (
          <div className="space-y-2">
            {transactions.transactions.map((tx) => (
              <button
                key={tx.id}
                onClick={() => router.push(`/payments/${tx.id}`)}
                className="w-full card p-4 hover:bg-neutral-50 dark:hover:bg-neutral-900 transition-colors text-left"
              >
                <div className="flex justify-between items-start gap-4">
                  <div className="flex-1">
                    <div className="font-medium">
                      {tx.source_currency} → {tx.destination_currency}
                    </div>
                    <div className="text-sm text-muted">
                      {new Date(tx.created_at).toLocaleDateString()}
                    </div>
                  </div>

                  <div className="text-right">
                    <div className="font-semibold">
                      {formatMoneyWithCurrency(tx.source_amount, tx.source_currency)}
                    </div>
                    <div className={`text-xs px-2 py-1 rounded-full inline-block ${statusBadgeColor(tx.status)}`}>
                      {tx.status.replace(/_/g, ' ')}
                    </div>
                  </div>
                </div>
              </button>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}
