'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, Copy, ExternalLink, Printer } from 'lucide-react';
import { StatusPill } from '@/components/ui/StatusPill';
import { useCountdown } from '@/hooks/useCountdown';
import { useCurrencies } from '@/hooks/useCurrencies';
import { usePaymentDetails } from '@/hooks/usePayment';
import { getPayoutInstitutions, payPayment, refundPayment } from '@/lib/api';
import type { ApiError } from '@/lib/api/errors';
import type { CurrencyInfo, PaymentDetails, PaymentStatus, StellarTx } from '@/lib/api/schemas';
import { NgBankRecipient } from '@/features/recipient/NgBankRecipient';
import { KePhoneRecipient } from '@/features/recipient/KePhoneRecipient';
import { MobileMoneyRecipient } from '@/features/recipient/MobileMoneyRecipient';
import type { RecipientDraft } from '@/features/recipient/types';
import { formatMinor, formatWithCode } from '@/lib/money';
import { TRACKER_STEPS, activeStepIndex, isReversal } from '@/lib/payment-status';
import { RAIL_LABELS } from '@/lib/rails';
import { formatRecipientAccount } from '@/lib/recipient-display';

const TRACKER_LABELS = ['Transfer received', 'Settled on Stellar', 'Paying recipient', 'Delivered'];
const PRE_PAYMENT: PaymentStatus[] = ['CREATED', 'QUOTED', 'AWAITING_PAYMENT'];

function expOf(currencies: CurrencyInfo[], code: string): number {
  return currencies.find((c) => c.code === code)?.exponent ?? 0;
}

export function PaymentView({ id }: { id: string }) {
  const query = usePaymentDetails(id);
  const currencies = useCurrencies();
  const error = query.error ?? currencies.error;

  if (error) {
    return (
      <p role="alert" className="notice notice-failed">
        {error.message}
      </p>
    );
  }
  const payment = query.data;
  const list = currencies.data;
  if (!payment || !list) return <p className="label">Loading payment…</p>;
  return <PaymentBody payment={payment} currencies={list} />;
}

function PaymentBody({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const queryClient = useQueryClient();
  const needsInvoice = PRE_PAYMENT.includes(payment.status) && payment.deposit === null;
  const requested = useRef(false);
  const invoice = useMutation({
    mutationFn: () => payPayment(payment.id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['payment', payment.id] }),
  });

  useEffect(() => {
    if (needsInvoice && !requested.current) {
      requested.current = true;
      invoice.mutate();
    }
  }, [needsInvoice, invoice]);

  if (PRE_PAYMENT.includes(payment.status)) {
    if (payment.status === 'AWAITING_PAYMENT' && payment.deposit) {
      return <DepositView payment={payment} currencies={currencies} />;
    }
    if (invoice.isError) {
      return (
        <p role="alert" className="notice notice-failed">
          {invoice.error.message}
        </p>
      );
    }
    return <p className="label">Preparing your payment details…</p>;
  }

  return (
    <div className="flex flex-col gap-5">
      <Outcome payment={payment} currencies={currencies} />
      {activeStepIndex(payment.status) !== null && <Tracker payment={payment} />}
      {payment.status === 'REFUND_PENDING' && <RefundPanel payment={payment} currencies={currencies} />}
      {payment.status === 'REFUNDED' && <Refunded payment={payment} currencies={currencies} />}
      {payment.status === 'COMPLETED' && <Receipt payment={payment} currencies={currencies} />}
      {payment.stellar_transactions.length > 0 && <StellarTrail payment={payment} currencies={currencies} />}
      <Details payment={payment} />
    </div>
  );
}

function CopyRow({ label, value, copyValue }: { label: string; value: string; copyValue?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(copyValue ?? value);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className="row-kv items-center">
      <dt>{label}</dt>
      <dd className="money flex items-center gap-2">
        <span>{value}</span>
        <button type="button" onClick={copy} className="btn btn-ghost" aria-label={`Copy ${label.toLowerCase()}`}>
          {copied ? <Check className="h-4 w-4" aria-hidden="true" /> : <Copy className="h-4 w-4" aria-hidden="true" />}
        </button>
      </dd>
    </div>
  );
}

function DepositView({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const deposit = payment.deposit;
  const { secondsLeft, isExpired } = useCountdown(deposit?.expires_at);

  if (!deposit) return null;
  if (isExpired) return <Expired payment={payment} />;

  const exponent = expOf(currencies, deposit.currency);
  const mobile = deposit.method === 'mobile_money';
  const total = Math.max(1, (new Date(deposit.expires_at).getTime() - new Date(deposit.created_at).getTime()) / 1000);
  const fraction = Math.min(1, Math.max(0, secondsLeft / total));
  const circumference = 2 * Math.PI * 22;
  const minutes = Math.ceil(secondsLeft / 60);

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col items-center gap-2 text-center">
        <p className="label">{mobile ? 'Pay exactly' : 'Transfer exactly'}</p>
        <p className="money text-[44px] font-semibold leading-none">
          {formatMinor(deposit.expected_amount, exponent)}
          <span className="ml-2 text-lg text-[var(--text-2)]">{deposit.currency}</span>
        </p>
        <p className="label">
          {payment.recipient_name ?? 'The recipient'} gets{' '}
          {formatWithCode(payment.expected_payout, expOf(currencies, payment.destination_currency), payment.destination_currency)}
        </p>
      </div>

      <section aria-labelledby="bank-details" className="card flex flex-col gap-2">
        <h2 id="bank-details" className="label font-semibold">
          {mobile ? 'Pay with mobile money' : 'Send a bank transfer to'}
        </h2>
        <dl className="m-0">
          {mobile ? (
            <>
              <div className="row-kv">
                <dt>Pay with</dt>
                <dd>{deposit.bank_name}</dd>
              </div>
              <CopyRow label={deposit.bank_name.includes('Paybill') ? 'Paybill number' : 'Merchant number'} value={deposit.account_number} />
              <CopyRow label="Reference" value={deposit.reference} />
              <div className="row-kv">
                <dt>Merchant name</dt>
                <dd>{deposit.account_name}</dd>
              </div>
            </>
          ) : (
            <>
              <CopyRow label="Account number" value={deposit.account_number} />
              <div className="row-kv">
                <dt>Bank</dt>
                <dd>{deposit.bank_name}</dd>
              </div>
              <div className="row-kv">
                <dt>Account name</dt>
                <dd>{deposit.account_name}</dd>
              </div>
            </>
          )}
          <CopyRow
            label="Amount"
            value={formatWithCode(deposit.expected_amount, exponent, deposit.currency)}
            copyValue={formatMinor(deposit.expected_amount, exponent).replace(/,/g, '')}
          />
        </dl>
        <p className="label">
          {mobile
            ? 'Use this reference as the account or payment reference, so we know the payment is yours. Send the exact amount in one payment.'
            : 'This account number is for this payment only. Send the exact amount in one transfer.'}
        </p>
      </section>

      <div className="flex items-center justify-center gap-3 pulse" aria-live="polite">
        <svg width="52" height="52" viewBox="0 0 52 52" aria-hidden="true">
          <circle cx="26" cy="26" r="22" fill="none" stroke="var(--raised)" strokeWidth="4" />
          <circle
            cx="26"
            cy="26"
            r="22"
            fill="none"
            stroke="var(--accent)"
            strokeWidth="4"
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={circumference * (1 - fraction)}
            transform="rotate(-90 26 26)"
          />
        </svg>
        <span className="text-left">
          <span className="block font-semibold">Waiting for your payment</span>
          <span className="label">{secondsLeft <= 60 ? `Expires in ${secondsLeft}s` : `Valid for ${minutes} min`}</span>
        </span>
      </div>

      <p className="label text-center">Keep this page open. We confirm as soon as your payment arrives.</p>
    </div>
  );
}

function Outcome({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  switch (payment.status) {
    case 'PAYMENT_DETECTED':
    case 'SETTLED':
    case 'PAYOUT_PENDING':
      return (
        <Notice tone="pending" pill={<StatusPill tone="pending" label="Payment received" />}>
          {payment.status === 'PAYOUT_PENDING'
            ? `Converted on Stellar. Sending to ${payment.recipient_name ?? 'the recipient'} now. This can take a few minutes depending on the provider.`
            : `Your transfer arrived. Converting to ${payment.destination_currency} on Stellar now.`}
          {payment.payout?.needs_review && (
            <span className="mt-2 block">We are reviewing this payment before it is delivered. {payment.payout.review_reason ?? ''}</span>
          )}
        </Notice>
      );
    case 'COMPLETED':
      return (
        <>
          <div className="success-check" aria-hidden="true">
            <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round">
              <path d="M5 12.5l4.5 4.5L19 7.5" />
            </svg>
          </div>
        <Notice tone="success" pill={<StatusPill tone="success" label="Money delivered" />}>
          {payment.recipient_name ?? 'The recipient'} has been paid{' '}
          {formatWithCode(payment.expected_payout, expOf(currencies, payment.destination_currency), payment.destination_currency)}.
        </Notice>
        </>
      );
    case 'EXPIRED':
      return <Expired payment={payment} />;
    case 'FAILED':
      if (isReversal(payment)) {
        return (
          <Notice tone="failed" pill={<StatusPill tone="failed" label="Payment reversed" />}>
            The payout partner reversed this payment after it had been reported as delivered. No money is lost. Your refund will be prepared next.
          </Notice>
        );
      }
      return (
        <Notice tone="failed" pill={<StatusPill tone="failed" label="Payment failed" />}>
          {payment.failure_reason ?? 'The payment could not be completed.'} Your refund will be prepared next.
        </Notice>
      );
    case 'REFUND_PENDING':
      if (payment.payout?.status === 'reversed') {
        return (
          <Notice tone="failed" pill={<StatusPill tone="failed" label="Payment reversed" />}>
            The payout partner reversed this payment after it had been reported as delivered. Your money is being refunded.
          </Notice>
        );
      }
      return (
        <Notice tone="pending" pill={<StatusPill tone="pending" label="Refund needed" />}>
          {payment.failure_reason ?? 'This payment could not be completed.'} Your money is being refunded.
        </Notice>
      );
    case 'REFUNDED':
      return (
        <Notice tone="refunded" pill={<StatusPill tone="refunded" label="Refunded" />}>
          Your refund was sent{payment.refund?.completed_at ? ` on ${new Date(payment.refund.completed_at).toLocaleString()}` : ''}.
        </Notice>
      );
    default:
      return null;
  }
}

function Notice({
  tone,
  pill,
  children,
}: {
  tone: 'pending' | 'success' | 'failed' | 'refunded';
  pill: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section role="status" aria-live="polite" className={`notice notice-${tone} flex-col`}>
      <div>{pill}</div>
      <div>{children}</div>
    </section>
  );
}

function Expired({ payment }: { payment: PaymentDetails }) {
  return (
    <Notice tone="pending" pill={<StatusPill tone="pending" label="Payment expired" />}>
      No transfer arrived in time, so nothing was sent. If you transferred after this, the money is refunded to you automatically. Start a new quote to try again with the same amount.
      <Link href={`/?from=${encodeURIComponent(payment.id)}`} className="btn btn-accent btn-block mt-3">
        Start a new quote
      </Link>
    </Notice>
  );
}

function Tracker({ payment }: { payment: PaymentDetails }) {
  const active = activeStepIndex(payment.status) ?? -1;
  return (
    <section aria-labelledby="tracker" className="card">
      <h2 id="tracker" className="label mb-4">
        Progress
      </h2>
      <ol className="stepper">
        {TRACKER_STEPS.map((step, index) => {
          const done = index < active || payment.status === 'COMPLETED';
          const current = index === active && payment.status !== 'COMPLETED';
          const cls = done ? 'step step-done' : current ? 'step step-current' : 'step step-todo';
          return (
            <li key={step} className={cls} aria-current={current ? 'step' : undefined}>
              <span className="step-dot" aria-hidden="true">
                {done ? '✓' : index + 1}
              </span>
              <span>
                {TRACKER_LABELS[index]}
                <span className="sr-only">{done ? ' (done)' : current ? ' (in progress)' : ''}</span>
              </span>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

function RefundPanel({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const queryClient = useQueryClient();
  const [formKey, setFormKey] = useState(0);
  const refund = payment.refund;
  const amount = refund?.amount ?? payment.deposit?.received_amount ?? payment.source_amount;
  const exponent = expOf(currencies, payment.source_currency);
  const start = useMutation({
    mutationFn: (draft: RecipientDraft) => refundPayment(payment.id, draft.recipient),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['payment', payment.id] }),
    onError: () => setFormKey((k) => k + 1),
  });
  const serverError = start.error as ApiError | Error | null;

  if (refund && refund.status !== 'failed') {
    return (
      <section aria-labelledby="refund" className="card flex flex-col gap-3">
        <h2 id="refund" className="text-lg font-semibold">
          Your refund
        </h2>
        <p className="label">
          Refunding <strong className="money text-[var(--text)]">{formatWithCode(amount, exponent, payment.source_currency)}</strong> to{' '}
          {refund.account_name ?? 'your account'} ({refund.destination_reference}). Refunds usually arrive within minutes.
        </p>
      </section>
    );
  }

  return (
    <section aria-labelledby="refund" className="card flex flex-col gap-3">
      <h2 id="refund" className="text-lg font-semibold">
        Get your refund
      </h2>
      <p className="label">
        {refund?.status === 'failed'
          ? `The refund to ${refund.destination_reference} did not go through${refund.failure_reason ? ` (${refund.failure_reason})` : ''}. Choose another account.`
          : 'Your bank or mobile money provider did not tell us which account you paid from.'}{' '}
        We will send <strong className="money text-[var(--text)]">{formatWithCode(amount, exponent, payment.source_currency)}</strong> to the{' '}
        {payment.source_currency} account you choose.
      </p>
      <RefundAccountForm key={formKey} payment={payment} currencies={currencies} onConfirm={(draft) => start.mutate(draft)} />
      {start.isPending && <p className="label">Starting your refund…</p>}
      {serverError && (
        <p role="alert" className="error-text">
          {serverError.message}
        </p>
      )}
    </section>
  );
}

// The refund goes out on the source currency's own rail: NGN bank, M-Pesa, or mobile money.
function RefundAccountForm({
  payment,
  currencies,
  onConfirm,
}: {
  payment: PaymentDetails;
  currencies: CurrencyInfo[];
  onConfirm: (draft: RecipientDraft) => void;
}) {
  const rails = currencies.find((c) => c.code === payment.source_currency)?.payout_rails ?? [];
  const rail = rails[0];
  if (!rail) return <p className="label">Refunds in {payment.source_currency} are not available yet. Contact support.</p>;
  if (rail.rail === 'NG_BANK_TRANSFER') return <NgBankRecipient onConfirm={onConfirm} />;
  if (rail.rail === 'KE_MOBILE_MONEY') return <KePhoneRecipient onConfirm={onConfirm} />;
  return <MobileMoneyRecipient currency={payment.source_currency} countries={rail.countries ?? []} onConfirm={onConfirm} />;
}

function Refunded({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const refund = payment.refund;
  if (!refund) return null;
  return (
    <section aria-labelledby="refunded" className="card flex flex-col gap-2">
      <h2 id="refunded" className="text-lg font-semibold">
        Refund details
      </h2>
      <p className="label">
        Refunded {formatWithCode(refund.amount, expOf(currencies, refund.currency), refund.currency)} to {refund.account_name ?? 'your account'} (
        {refund.destination_reference}){refund.completed_at ? ` on ${new Date(refund.completed_at).toLocaleString()}` : ''}.
      </p>
    </section>
  );
}

const STELLAR_LABELS: Record<StellarTx['kind'], string> = {
  settle: 'Minted and converted',
  redeem: 'Tokens burned after payout',
  reverse: 'Conversion reversed',
};

function stroops(amount: number | null): string {
  if (amount === null) return '';
  return (amount / 1e7).toLocaleString('en-US', { maximumFractionDigits: 2 });
}

function StellarTrail({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  return (
    <section aria-labelledby="stellar" className="card flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h2 id="stellar" className="text-lg font-semibold">
          On Stellar
        </h2>
        <span className="label">{payment.ledger === 'simulated' ? 'Simulated ledger' : 'Stellar testnet'}</span>
      </div>
      <p className="label">
        {formatWithCode(payment.send_amount, expOf(currencies, payment.source_currency), payment.source_currency)} converted along{' '}
        {payment.conversion_path.join(' → ')} on the Stellar DEX, in one atomic transaction.
      </p>
      <ol className="m-0 flex flex-col gap-3 p-0">
        {payment.stellar_transactions.map((tx) => (
          <li key={tx.tx_hash} className="flex flex-col gap-1 border-l-2 border-[var(--hairline)] pl-3">
            <span className="flex items-center gap-2">
              <span className="font-medium">{STELLAR_LABELS[tx.kind]}</span>
              <StatusPill
                tone={tx.status === 'succeeded' ? 'success' : tx.status === 'failed' ? 'failed' : 'pending'}
                label={tx.status === 'succeeded' ? 'Confirmed' : tx.status === 'failed' ? 'Failed' : 'Submitted'}
              />
            </span>
            {tx.kind === 'settle' && tx.delivered !== null && (
              <span className="label money">
                Delivered {stroops(tx.delivered)} {payment.destination_currency}
              </span>
            )}
            {tx.error && <span className="label">{tx.error}</span>}
            {tx.explorer_url ? (
              <a href={tx.explorer_url} target="_blank" rel="noreferrer" className="label inline-flex items-center gap-1 underline">
                <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
                {tx.tx_hash.slice(0, 12)}…{tx.ledger ? ` · ledger ${tx.ledger}` : ''}
              </a>
            ) : (
              <span className="label font-mono">{tx.tx_hash.slice(0, 16)}…</span>
            )}
          </li>
        ))}
      </ol>
    </section>
  );
}

function Receipt({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const payout = payment.payout;
  const showsInstitution = payment.recipient_rail === 'NG_BANK_TRANSFER' || payment.recipient_rail === 'MOBILE_MONEY';
  const institutions = useQuery({
    queryKey: ['institutions', payment.destination_currency, payment.recipient_rail],
    queryFn: () => getPayoutInstitutions(payment.destination_currency, payment.recipient_rail),
    enabled: showsInstitution,
    staleTime: 60 * 60 * 1000,
  });
  return (
    <section aria-labelledby="receipt" className="card flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <h2 id="receipt" className="text-lg font-semibold">
          Receipt
        </h2>
        <button type="button" onClick={() => window.print()} className="btn btn-ghost">
          <Printer className="h-4 w-4" aria-hidden="true" />
          Download receipt
        </button>
      </div>
      <p className="money text-[40px] font-semibold leading-none">
        {formatMinor(payment.expected_payout, expOf(currencies, payment.destination_currency))}
        <span className="ml-2 text-lg text-[var(--text-2)]">{payment.destination_currency}</span>
      </p>
      <dl className="m-0">
        <Row label="You sent" value={formatWithCode(payment.source_amount, expOf(currencies, payment.source_currency), payment.source_currency)} />
        <Row label="Fee" value={formatWithCode(payment.fee_source_amount, expOf(currencies, payment.source_currency), payment.source_currency)} />
        <Row label="Rate" value={`1 ${payment.source_currency} ≈ ${payment.exchange_rate} ${payment.destination_currency}`} />
        <Row label="Route" value={`Stellar ${payment.conversion_path.join(' → ')}`} />
        <Row label="Recipient" value={payment.recipient_name ?? 'Not checked'} />
        <Row label="Account" value={formatRecipientAccount(payment.recipient_masked, payment.recipient_rail, institutions.data)} />
        <Row label="Method" value={RAIL_LABELS[payment.recipient_rail]} />
        {payout?.provider_reference && <Row label="Reference" value={payout.provider_reference} />}
        <Row label="Created" value={new Date(payment.created_at).toLocaleString()} />
        {payout?.completed_at && <Row label="Delivered" value={new Date(payout.completed_at).toLocaleString()} />}
      </dl>
      <Link href="/" className="btn btn-quiet btn-block">
        Send another
      </Link>
    </section>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="row-kv">
      <dt>{label}</dt>
      <dd className="money">{value}</dd>
    </div>
  );
}

function Details({ payment }: { payment: PaymentDetails }) {
  return (
    <details className="card text-sm">
      <summary className="cursor-pointer font-medium">Details</summary>
      <p className="label mt-2">
        Status: <span className="font-mono">{payment.status}</span>
        {payment.payout ? ` · payout ${payment.payout.status}` : ''}
      </p>
      <ol className="mt-3 flex flex-col gap-3">
        {payment.events.map((event) => (
          <li key={event.id} className="border-l-2 border-[var(--hairline)] pl-3">
            <p className="font-medium">{event.event_type.replace(/\./g, ' ')}</p>
            <p className="label">
              {new Date(event.created_at).toLocaleString()}
              {event.to_status ? ` · ${event.to_status}` : ''}
            </p>
          </li>
        ))}
      </ol>
    </details>
  );
}
