'use client';

import { useState } from 'react';
import { useCreatePayment } from '@/hooks/usePayment';
import { useQuote } from '@/hooks/useQuote';
import { useCurrencies } from '@/hooks/useCurrencies';
import { formatMoneyWithCurrency } from '@/lib/money';
import { NgBankRecipient } from '@/features/recipient/NgBankRecipient';
import { LightningAddressRecipient } from '@/features/recipient/LightningAddressRecipient';
import { KePhoneRecipient } from '@/features/recipient/KePhoneRecipient';
import { ChevronLeft, AlertCircle, CheckCircle } from 'lucide-react';
import type { Currency, Recipient, Rail } from '@/lib/api/schemas';

interface ReviewPageProps {
  quoteId: string;
  sourceCurrency: Currency;
  destCurrency: Currency;
  onPaymentCreated: (paymentId: string) => void;
  onBack: () => void;
}

export function ReviewPage({
  quoteId,
  sourceCurrency,
  destCurrency,
  onPaymentCreated,
  onBack,
}: ReviewPageProps) {
  const { data: quote } = useQuote(quoteId);
  const { data: currencies = [] } = useCurrencies();
  const createPaymentMutation = useCreatePayment();

  const [step, setStep] = useState<'recipient' | 'confirm'>('recipient');
  const [recipient, setRecipient] = useState<Recipient | null>(null);
  const [recipientName, setRecipientName] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);

  if (!quote) {
    return (
      <div className="container-safe max-w-md">
        <p className="text-muted text-center">Loading quote...</p>
      </div>
    );
  }

  const getRailRenderer = (rail: Rail) => {
    switch (rail) {
      case 'NG_BANK_TRANSFER':
        return (
          <NgBankRecipient
            currency={destCurrency}
            onResolve={(data) => {
              setRecipient({
                rail: 'NG_BANK_TRANSFER',
                bank_code: data.bank_code,
                account_number: data.account_number,
                resolution_id: data.resolution_id,
              });
              setRecipientName(data.account_name);
              setStep('confirm');
            }}
          />
        );
      case 'LIGHTNING_ADDRESS':
        return (
          <LightningAddressRecipient
            onSubmit={(address) => {
              setRecipient({
                rail: 'LIGHTNING_ADDRESS',
                address,
              });
              setRecipientName(null);
              setStep('confirm');
            }}
          />
        );
      case 'KE_MOBILE_MONEY':
        return (
          <KePhoneRecipient
            onSubmit={(phoneNumber) => {
              setRecipient({
                rail: 'KE_MOBILE_MONEY',
                phone_number: phoneNumber,
              });
              setRecipientName(null);
              setStep('confirm');
            }}
          />
        );
      default:
        return <p className="text-error-600">Unsupported payment method</p>;
    }
  };

  const handlePayment = () => {
    if (!recipient) return;

    createPaymentMutation.mutate(
      {
        quote_id: quoteId,
        recipient,
      },
      {
        onSuccess: (payment) => {
          onPaymentCreated(payment.id);
        },
      }
    );
  };

  return (
    <div className="container-safe max-w-md">
      {/* Header */}
      <div className="flex items-center gap-2 mb-8">
        <button
          onClick={onBack}
          className="btn btn-ghost p-2"
          aria-label="Go back"
        >
          <ChevronLeft className="w-5 h-5" />
        </button>
        <h1 className="text-2xl font-bold">
          {step === 'recipient' ? 'Recipient details' : 'Review & confirm'}
        </h1>
      </div>

      {/* Recipient input */}
      {step === 'recipient' && (
        <div className="card card-content space-y-4">
          {getRailRenderer(quote.destination_rail)}
        </div>
      )}

      {/* Review summary */}
      {step === 'confirm' && recipient && (
        <div className="space-y-4">
          <div className="card card-content space-y-4">
            <div>
              <p className="text-sm text-muted mb-1">You send</p>
              <p className="text-2xl font-bold">
                {formatMoneyWithCurrency(quote.source_amount, quote.source_currency)}
              </p>
            </div>

            <div className="border-t border-neutral-200 dark:border-neutral-700 pt-4">
              <p className="text-sm text-muted mb-1">Fee</p>
              <p className="text-lg font-semibold">
                {formatMoneyWithCurrency(quote.fee_source_amount, quote.source_currency)}
              </p>
            </div>

            <div className="border-t border-neutral-200 dark:border-neutral-700 pt-4">
              <p className="text-sm text-muted mb-1">They receive</p>
              <p className="text-2xl font-bold">
                {formatMoneyWithCurrency(quote.destination_amount, quote.destination_currency)}
              </p>
            </div>

            <div className="border-t border-neutral-200 dark:border-neutral-700 pt-4">
              <p className="text-sm text-muted mb-1">Exchange rate</p>
              <p className="text-sm">
                1 {quote.source_currency} = {quote.exchange_rate} {quote.destination_currency}
              </p>
            </div>
          </div>

          {/* Recipient card */}
          <div className="card card-content">
            <p className="text-sm text-muted mb-2">Recipient</p>
            {recipientName ? (
              <div className="space-y-1">
                <p className="font-medium">{recipientName}</p>
                <p className="text-sm text-muted">{quote.destination_rail.replace(/_/g, ' ')}</p>
              </div>
            ) : (
              <p className="text-sm text-muted">{quote.destination_rail.replace(/_/g, ' ')}</p>
            )}
          </div>

          {/* Error */}
          {createPaymentMutation.isError && (
            <div className="flex gap-2 p-3 rounded-lg bg-error-50 dark:bg-error-950 text-error-700 dark:text-error-300">
              <AlertCircle className="w-5 h-5 flex-shrink-0 mt-0.5" />
              <p className="text-sm">{createPaymentMutation.error?.message}</p>
            </div>
          )}

          {/* Confirm button */}
          <div className="space-y-2">
            <button
              onClick={handlePayment}
              disabled={createPaymentMutation.isPending || !confirmed}
              className="btn btn-primary w-full"
            >
              {createPaymentMutation.isPending ? 'Creating payment...' : 'Continue to payment'}
            </button>

            {/* Checkbox for confirmation */}
            <label className="flex items-start gap-2 p-3 rounded-lg bg-neutral-50 dark:bg-neutral-900 cursor-pointer">
              <input
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
                className="mt-0.5"
              />
              <span className="text-sm">
                I confirm the recipient details are correct.
              </span>
            </label>
          </div>

          {/* Back button */}
          <button
            onClick={() => setStep('recipient')}
            className="btn btn-secondary w-full"
          >
            Change recipient
          </button>
        </div>
      )}
    </div>
  );
}
