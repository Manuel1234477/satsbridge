'use client';

import { useRouter } from 'next/navigation';
import { usePaymentDetails } from '@/hooks/usePayment';
import { PaymentQr } from '@/features/payment/PaymentQr';
import { formatMoneyWithCurrency } from '@/lib/money';
import { AlertCircle, CheckCircle, Clock } from 'lucide-react';

export default function PaymentPage({ params }: { params: { id: string } }) {
  const router = useRouter();
  const { data: payment, isLoading, error } = usePaymentDetails(params.id);

  if (isLoading) {
    return (
      <div className="container-safe max-w-md py-12 text-center">
        <p className="text-muted">Loading payment details...</p>
      </div>
    );
  }

  if (error || !payment) {
    return (
      <div className="container-safe max-w-md py-12">
        <div className="flex gap-3 p-4 rounded-lg bg-error-50 dark:bg-error-950 border border-error-200 dark:border-error-800">
          <AlertCircle className="w-5 h-5 text-error-600 dark:text-error-400 flex-shrink-0 mt-0.5" />
          <div>
            <p className="font-medium text-error-900 dark:text-error-100">Payment not found</p>
            <p className="text-sm text-error-700 dark:text-error-300">{error?.message}</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <main className="min-h-screen bg-white dark:bg-neutral-950 py-8">
      <div className="container-safe max-w-md">
        {payment.status === 'AWAITING_PAYMENT' && (
          <>
            <h1 className="text-3xl font-bold mb-8 text-center">Send payment</h1>
            <PaymentQr payment={payment} />
          </>
        )}

        {payment.status === 'COMPLETED' && (
          <div className="text-center space-y-6 py-12">
            <div className="flex justify-center">
              <CheckCircle className="w-16 h-16 text-success-600 animate-bounce" />
            </div>
            <h1 className="text-3xl font-bold">Payment complete!</h1>
            <div className="card card-content space-y-3">
              <div>
                <p className="text-sm text-muted">You sent</p>
                <p className="font-bold text-lg">{formatMoneyWithCurrency(payment.source_amount, payment.source_currency)}</p>
              </div>
              <div>
                <p className="text-sm text-muted">
                  {payment.recipient_name || 'Recipient'} received
                </p>
                <p className="font-bold text-lg">{formatMoneyWithCurrency(payment.expected_payout, payment.destination_currency)}</p>
              </div>
              {payment.payout && (
                <div className="border-t border-neutral-200 dark:border-neutral-700 pt-3">
                  <p className="text-xs text-muted">{payment.payout.provider_reference && `Reference: ${payment.payout.provider_reference}`}</p>
                </div>
              )}
            </div>
            <button
              onClick={() => router.push('/')}
              className="btn btn-primary w-full"
            >
              Send another payment
            </button>
          </div>
        )}

        {['PAYMENT_DETECTED', 'SETTLED'].includes(payment.status) && (
          <div className="text-center space-y-6 py-12">
            <Clock className="w-12 h-12 mx-auto text-primary-600 animate-spin" />
            <p className="text-lg font-medium">Payment processing...</p>
            <p className="text-muted text-sm">{payment.status.replace(/_/g, ' ')}</p>
            <div className="card card-content text-sm text-muted">
              We received your Lightning payment. Converting to {payment.destination_currency} now.
            </div>
          </div>
        )}

        {payment.status === 'PAYOUT_PENDING' && (
          <div className="text-center space-y-6 py-12">
            <Clock className="w-12 h-12 mx-auto text-primary-600 animate-spin" />
            <p className="text-lg font-medium">Sending to recipient...</p>
            <p className="text-muted text-sm">This usually takes a few minutes</p>
            <div className="card card-content space-y-2 text-sm">
              <p className="text-muted">
                Bank transfers can take time depending on business hours and the destination bank.
              </p>
              {payment.payout?.needs_review && (
                <p className="text-warning-700 dark:text-warning-300">
                  We're reviewing this payment for security.
                </p>
              )}
            </div>
          </div>
        )}

        {payment.status === 'EXPIRED' && (
          <div className="card p-6 text-center space-y-4">
            <AlertCircle className="w-12 h-12 mx-auto text-warning-600" />
            <p className="text-warning-700 dark:text-warning-300">Invoice expired</p>
            <p className="text-sm text-muted">The Lightning invoice is no longer valid.</p>
            <button
              onClick={() => router.push('/')}
              className="btn btn-primary w-full"
            >
              Start a new quote
            </button>
          </div>
        )}

        {payment.status === 'FAILED' && (
          <div className="card p-6 text-center space-y-4">
            <AlertCircle className="w-12 h-12 mx-auto text-error-600" />
            <p className="text-error-700 dark:text-error-300 font-medium">Payment failed</p>
            {payment.failure_reason && (
              <p className="text-sm text-muted">{payment.failure_reason}</p>
            )}
            <button
              onClick={() => router.push('/')}
              className="btn btn-primary w-full"
            >
              Try again
            </button>
          </div>
        )}
      </div>
    </main>
  );
}
