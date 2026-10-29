'use client';

import { useState } from 'react';
import { ChevronLeft } from 'lucide-react';
import { useMutation } from '@tanstack/react-query';
import { createPayment, payPayment } from '@/lib/api';
import type { ApiError } from '@/lib/api/errors';
import type { CurrencyInfo, Quote, Rail } from '@/lib/api/schemas';
import { formatMinor, formatWithCode } from '@/lib/money';
import { RAIL_LABELS } from '@/lib/rails';
import { RailSelector } from '@/components/ui/RailSelector';
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
    <div className="page narrow">
      <div className="flex items-center gap-2">
        <button type="button" onClick={onBack} className="btn btn-ghost" aria-label="Back to amounts">
          <ChevronLeft className="h-5 w-5" aria-hidden="true" />
        </button>
        <h1 className="page-title">{draft ? 'Review and pay' : 'Who is receiving this?'}</h1>
      </div>

      {!draft && (
        <div className="flex flex-col gap-2">
          <RailSelector
            rails={currencies.find((c) => c.code === quote.destination_currency)?.payout_rails.map((r) => r.rail) ?? [rail]}
            value={rail}
            label="Payout method for this quote"
          />
          <p className="label">The payout method is part of your quote. Go back to change it.</p>
        </div>
      )}

      {!draft && (
        <section className="card">
          {rail === 'NG_BANK_TRANSFER' && <NgBankRecipient onConfirm={setDraft} />}
          {rail === 'LIGHTNING_ADDRESS' && <LightningAddressRecipient onConfirm={setDraft} />}
          {rail === 'KE_MOBILE_MONEY' && <KePhoneRecipient onConfirm={setDraft} />}
          {rail === 'MOBILE_MONEY' && <p className="label">This payout method is not available in this demo.</p>}
        </section>
      )}

      {draft && (
        <>
          <section aria-labelledby="summary" className="card flex flex-col gap-4">
            <h2 id="summary" className="label">
              They receive{quote.payout_amount_guaranteed ? '' : ' (estimated)'}
            </h2>
            <p className="money text-[44px] font-semibold leading-none">
              {formatMinor(quote.destination_amount, destinationExponent)}
              <span className="ml-2 text-lg text-[var(--text-2)]">{quote.destination_currency}</span>
            </p>
            <dl className="m-0">
              <div className="row-kv">
                <dt>You send</dt>
                <dd className="money">{formatWithCode(quote.source_amount, sourceExponent, quote.source_currency)}</dd>
              </div>
              <div className="row-kv">
                <dt>Fee</dt>
                <dd className="money">{formatWithCode(quote.fee_source_amount, sourceExponent, quote.source_currency)}</dd>
              </div>
              <div className="row-kv">
                <dt>Rate</dt>
                <dd className="money">
                  1 {quote.source_currency} ≈ {quote.exchange_rate} {quote.destination_currency}
                </dd>
              </div>
              <div className="row-kv">
                <dt>Recipient</dt>
                <dd>{draft.displayName ?? 'Name not checked'}</dd>
              </div>
              <div className="row-kv">
                <dt>Paid to</dt>
                <dd>{draft.summary}</dd>
              </div>
              <div className="row-kv">
                <dt>Method</dt>
                <dd>{RAIL_LABELS[quote.destination_rail]}</dd>
              </div>
            </dl>
          </section>

          {isExpired && (
            <p role="alert" className="notice notice-pending">
              This rate has expired, so we cannot take payment on it. Go back to get the latest rate.
            </p>
          )}
          {submitError && (
            <p role="alert" className="notice notice-failed">
              {expiredBeforeSubmit ? 'This rate has expired. Go back to get the latest rate.' : submitError.message}
            </p>
          )}

          <div className="dock">
            <button type="button" onClick={() => submit.mutate(draft)} disabled={isExpired || submit.isPending} className="btn btn-accent btn-block">
              {submit.isPending ? 'Creating payment…' : paymentId ? 'Try getting the invoice again' : 'Confirm and continue'}
            </button>
            <button type="button" onClick={() => setDraft(null)} disabled={submit.isPending} className="btn btn-quiet btn-block">
              Change recipient
            </button>
          </div>
        </>
      )}
    </div>
  );
}
