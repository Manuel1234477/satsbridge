'use client';

import { useState } from 'react';
import { ChevronLeft } from 'lucide-react';
import { useMutation } from '@tanstack/react-query';
import { createPayment, payPayment } from '@/lib/api';
import type { ApiError } from '@/lib/api/errors';
import type { CurrencyInfo, Quote, Rail } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
import { RAIL_LABELS } from '@/lib/rails';
import { useCountdown } from '@/hooks/useCountdown';
import { NgBankRecipient } from '@/features/recipient/NgBankRecipient';
import { LightningAddressRecipient } from '@/features/recipient/LightningAddressRecipient';
import { KePhoneRecipient } from '@/features/recipient/KePhoneRecipient';
import type { RecipientDraft } from '@/features/recipient/types';

interface ReviewPageProps {
  quote: Quote;
  rail: Rail;
  currencies: CurrencyInfo[];
  onBack: () => void;
  onPaid: (paymentId: string) => void;
}

export function ReviewPage({ quote, rail, currencies, onBack, onPaid }: ReviewPageProps) {
  const [draft, setDraft] = useState<RecipientDraft | null>(null);
  // Kept across retries so a failed /pay never creates a second payment.
  const [paymentId, setPaymentId] = useState<string | null>(null);
  const { isExpired } = useCountdown(quote.expires_at);

  const sourceExponent = currencies.find((c) => c.code === quote.source_currency)?.exponent ?? 0;
  const destinationExponent = currencies.find((c) => c.code === quote.destination_currency)?.exponent ?? 0;

  const submit = useMutation({
    mutationFn: async (input: RecipientDraft) => {
      if (isExpired) throw new Error('quote expired');
      let id = paymentId;
      if (!id) {
        const payment = await createPayment({ quote_id: quote.id, recipient: input.recipient });
        id = payment.id;
        setPaymentId(id);
      }
      await payPayment(id);
      return id;
    },
    onSuccess: (id) => onPaid(id),
  });

  const submitError = submit.error as ApiError | Error | null;
  const expiredBeforeSubmit = submitError?.message === 'quote expired';

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2">
        <button type="button" onClick={onBack} className="btn btn-ghost p-2" aria-label="Back to amounts">
          <ChevronLeft className="h-5 w-5" aria-hidden="true" />
        </button>
        <h1 className="text-2xl font-bold">{draft ? 'Review and pay' : 'Who is receiving this?'}</h1>
      </div>

      {!draft && (
        <section className="card card-content">
          {rail === 'NG_BANK_TRANSFER' && <NgBankRecipient onConfirm={setDraft} />}
          {rail === 'LIGHTNING_ADDRESS' && <LightningAddressRecipient onConfirm={setDraft} />}
          {rail === 'KE_MOBILE_MONEY' && <KePhoneRecipient onConfirm={setDraft} />}
          {rail === 'MOBILE_MONEY' && <p className="text-sm text-muted">This payout method is not available in this demo.</p>}
        </section>
      )}

      {draft && (
        <>
          <section aria-labelledby="summary" className="card card-content space-y-4">
            <h2 id="summary" className="sr-only">
              Payment summary
            </h2>
            <div>
              <p className="text-sm text-muted">You send</p>
              <p className="text-2xl font-bold tabular-nums">
                {formatWithCode(quote.source_amount, sourceExponent, quote.source_currency)}
              </p>
              <p className="text-xs text-muted">
                Fee {formatWithCode(quote.fee_source_amount, sourceExponent, quote.source_currency)} included
              </p>
            </div>
            <div className="border-t border-[var(--card-border)] pt-4">
              <p className="text-sm text-muted">They receive{quote.payout_amount_guaranteed ? '' : ' (estimated)'}</p>
              <p className="text-2xl font-bold tabular-nums">
                {formatWithCode(quote.destination_amount, destinationExponent, quote.destination_currency)}
              </p>
            </div>
            <dl className="space-y-2 border-t border-[var(--card-border)] pt-4 text-sm">
              <div className="flex justify-between gap-4">
                <dt className="text-muted">Recipient</dt>
                <dd className="text-right">{draft.displayName ?? 'Name not checked'}</dd>
              </div>
              <div className="flex justify-between gap-4">
                <dt className="text-muted">Paid to</dt>
                <dd className="text-right">{draft.summary}</dd>
              </div>
              <div className="flex justify-between gap-4">
                <dt className="text-muted">Method</dt>
                <dd className="text-right">{RAIL_LABELS[quote.destination_rail]}</dd>
              </div>
              <div className="flex justify-between gap-4">
                <dt className="text-muted">Rate</dt>
                <dd className="text-right tabular-nums">
                  1 {quote.source_currency} ≈ {quote.exchange_rate} {quote.destination_currency}
                </dd>
              </div>
            </dl>
          </section>

          {isExpired && (
            <p role="alert" className="rounded-lg bg-warning-50 px-3 py-2 text-sm text-warning-800 dark:bg-warning-950 dark:text-warning-100">
              This rate has expired, so we cannot take payment on it. Go back to get the latest rate.
            </p>
          )}
          {submitError && (
            <p role="alert" className="rounded-lg bg-error-50 px-3 py-2 text-sm text-error-700 dark:bg-error-950 dark:text-error-200">
              {expiredBeforeSubmit ? 'This rate has expired. Go back to get the latest rate.' : submitError.message}
            </p>
          )}

          <button
            type="button"
            onClick={() => submit.mutate(draft)}
            disabled={isExpired || submit.isPending}
            className="btn btn-primary w-full py-3 text-base"
          >
            {submit.isPending ? 'Creating payment…' : paymentId ? 'Try getting the invoice again' : 'Confirm and continue'}
          </button>
          <button type="button" onClick={() => setDraft(null)} disabled={submit.isPending} className="btn btn-secondary w-full">
            Change recipient
          </button>
        </>
      )}
    </div>
  );
}
