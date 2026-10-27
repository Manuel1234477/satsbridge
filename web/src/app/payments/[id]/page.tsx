'use client';

import { usePaymentDetails } from '@/hooks/usePayment';
import { PaymentQr } from '@/features/payment/PaymentQr';
import { AlertCircle, CheckCircle } from 'lucide-react';

export default function PaymentPage({ params }: { params: { id: string } }) {
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
              <CheckCircle className="w-16 h-16 text-success-600" />
            </div>
            <h1 className="text-3xl font-bold">Payment complete!</h1>
            <p className="text-lg text-muted">
              {payment.recipient_name} has received{' '}
              {payment.expected_payout.toLocaleString()} {payment.destination_currency}
            </p>
            <button className="btn btn-primary w-full">Send another</button>
          </div>
        )}

        {['PAYMENT_DETECTED', 'SETTLED', 'PAYOUT_PENDING'].includes(payment.status) && (
          <div className="text-center space-y-4 py-12">
            <p className="text-lg font-medium">Processing payment...</p>
            <p className="text-muted text-sm">Status: {payment.status.replace(/_/g, ' ')}</p>
            <div className="animate-spin mx-auto w-8 h-8 border-4 border-neutral-200 dark:border-neutral-700 border-t-primary-600 rounded-full" />
          </div>
        )}

        {payment.status === 'EXPIRED' && (
          <div className="card p-6 text-center space-y-4">
            <AlertCircle className="w-12 h-12 mx-auto text-warning-600" />
            <p className="text-warning-700 dark:text-warning-300">Invoice expired</p>
            <button className="btn btn-primary w-full">Start a new quote</button>
          </div>
        )}

        {payment.status === 'FAILED' && (
          <div className="card p-6 text-center space-y-4">
            <AlertCircle className="w-12 h-12 mx-auto text-error-600" />
            <p className="text-error-700 dark:text-error-300 font-medium">Payment failed</p>
            {payment.failure_reason && (
              <p className="text-sm text-muted">{payment.failure_reason}</p>
            )}
            <button className="btn btn-primary w-full">Try again</button>
          </div>
        )}
      </div>
    </main>
  );
}
