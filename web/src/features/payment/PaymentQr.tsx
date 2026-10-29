'use client';

import { useState } from 'react';
import { LightningQr } from '@/components/QrCode';
import { formatMoneyWithCurrency } from '@/lib/money';
import { useCountdown } from '@/hooks/useCountdown';
import { Copy, Check, AlertCircle } from 'lucide-react';
import type { PaymentDetails } from '@/lib/api/schemas';

interface PaymentQrProps {
  payment: PaymentDetails;
}

export function PaymentQr({ payment }: PaymentQrProps) {
  const [copied, setCopied] = useState(false);
  const { secondsLeft, isExpired } = useCountdown(payment.lightning?.expires_at || null);

  if (!payment.lightning) {
    return (
      <div className="card p-6 text-center">
        <AlertCircle className="w-12 h-12 mx-auto mb-4 text-error-600" />
        <p className="text-error-700 dark:text-error-300">Lightning invoice not available</p>
      </div>
    );
  }

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div className="space-y-6">
      {/* QR Code */}
      <div className="card">
        <div className="card-content">
          <LightningQr value={payment.lightning.payment_request} />
        </div>
      </div>

      {/* Amount in sats */}
      <div className="text-center">
        <div className="text-4xl font-bold tabular-nums">
          {payment.lightning.amount_sats.toLocaleString()} <span className="text-lg">sats</span>
        </div>
        <div className="text-sm text-muted mt-2">
          ≈ {formatMoneyWithCurrency(payment.source_amount, payment.source_currency)}
        </div>
      </div>

      {/* Invoice countdown */}
      <div className={`p-3 rounded-lg text-sm ${
        isExpired
          ? 'bg-error-50 dark:bg-error-950 text-error-700 dark:text-error-300'
          : secondsLeft <= 60
          ? 'bg-warning-50 dark:bg-warning-950 text-warning-700 dark:text-warning-300'
          : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300'
      }`}>
        {isExpired ? 'Invoice expired' : `Invoice expires in ${secondsLeft}s`}
      </div>

      {/* Copy buttons */}
      <div className="space-y-2">
        <button
          onClick={() => handleCopy(payment.lightning!.payment_request)}
          className="btn btn-secondary w-full flex items-center justify-center gap-2"
        >
          {copied ? (
            <>
              <Check className="w-4 h-4" />
              Copied!
            </>
          ) : (
            <>
              <Copy className="w-4 h-4" />
              Copy invoice
            </>
          )}
        </button>

        {payment.lnurl && (
          <button
            onClick={() => handleCopy(payment.lnurl!)}
            className="btn btn-secondary w-full text-sm"
          >
            Copy LNURL
          </button>
        )}
      </div>

      {/* Instructions */}
      <div className="bg-neutral-50 dark:bg-neutral-900 p-4 rounded-lg text-sm space-y-2">
        <p className="font-medium">Steps:</p>
        <ol className="list-decimal list-inside space-y-1 text-muted">
          <li>Open your Lightning wallet</li>
          <li>Scan this QR code or paste the invoice</li>
          <li>Confirm the payment</li>
          <li>We'll send the funds to {payment.recipient_name || 'the recipient'}</li>
        </ol>
      </div>
    </div>
  );
}
