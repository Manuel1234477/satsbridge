'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Check, Copy, ExternalLink, Printer } from 'lucide-react';
import { LightningQr } from '@/components/QrCode';
import { StatusPill } from '@/components/ui/StatusPill';
import { useCountdown } from '@/hooks/useCountdown';
import { useCurrencies } from '@/hooks/useCurrencies';
import { usePaymentDetails } from '@/hooks/usePayment';
import { payPayment, refundPayment, simulatePayout } from '@/lib/api';
import type { ApiError } from '@/lib/api/errors';
import type { CurrencyInfo, PaymentDetails, PaymentStatus } from '@/lib/api/schemas';
import { formatMinor, formatWithCode } from '@/lib/money';
import { TRACKER_STEPS, activeStepIndex, isReversal } from '@/lib/payment-status';
import { RAIL_LABELS } from '@/lib/rails';

const TRACKER_LABELS = ['Payment received', 'Converting', 'Paying recipient', 'Delivered'];
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
  const needsInvoice = PRE_PAYMENT.includes(payment.status) && payment.lightning === null;
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
    if (payment.status === 'AWAITING_PAYMENT' && payment.lightning) {
      return <InvoiceView payment={payment} currencies={currencies} />;
    }
    if (invoice.isError) {
      return (
        <p role="alert" className="notice notice-failed">
          {invoice.error.message}
        </p>
      );
    }
    return <p className="label">Preparing your Lightning invoice…</p>;
  }

  return (
    <div className="flex flex-col gap-5">
      <Outcome payment={payment} currencies={currencies} />
      {activeStepIndex(payment.status) !== null && <Tracker payment={payment} />}
      {payment.status === 'REFUND_PENDING' && <RefundForm payment={payment} />}
      {payment.status === 'REFUNDED' && <Refunded payment={payment} />}
      {payment.status === 'COMPLETED' && <Receipt payment={payment} currencies={currencies} />}
      <DevPayoutPanel payment={payment} />
      <Details payment={payment} />
    </div>
  );
}

function InvoiceView({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const invoice = payment.lightning;
  const [copied, setCopied] = useState<'invoice' | 'lnurl' | null>(null);
  const { secondsLeft, isExpired } = useCountdown(invoice?.expires_at);

  if (!invoice) return null;
  if (isExpired) return <Expired payment={payment} />;

  const invoiceSeconds = Math.max(1, (new Date(invoice.expires_at).getTime() - new Date(invoice.created_at).getTime()) / 1000);
  const fraction = Math.min(1, Math.max(0, secondsLeft / invoiceSeconds));
  const circumference = 2 * Math.PI * 22;

  const copy = async (text: string, which: 'invoice' | 'lnurl') => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      setTimeout(() => setCopied(null), 2000);
    } catch {
      setCopied(null);
    }
  };

  return (
    <div className="flex flex-col items-center gap-5 text-center">
      <p className="label">Pay {formatWithCode(payment.source_amount, expOf(currencies, payment.source_currency), payment.source_currency)}</p>
      <div className="qr-frame">
        <LightningQr value={invoice.payment_request} />
      </div>
      <p className="money text-[48px] font-semibold leading-none">
        {invoice.amount_sats.toLocaleString('en-US')}
        <span className="ml-2 text-lg text-[var(--text-2)]">sats</span>
      </p>
      <p className="label">
        ≈ {formatWithCode(payment.source_amount, expOf(currencies, payment.source_currency), payment.source_currency)}
      </p>

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
          <span className="block font-semibold">Waiting for payment</span>
          <span className="label">
            {secondsLeft <= 60 ? `Invoice expires in ${secondsLeft}s` : `Invoice valid for ${Math.ceil(secondsLeft / 60)} min`}
          </span>
        </span>
      </div>

      <div className="flex w-full flex-col gap-3">
        <button type="button" onClick={() => copy(invoice.payment_request, 'invoice')} className="btn btn-accent btn-block">
          {copied === 'invoice' ? <Check className="h-4 w-4" aria-hidden="true" /> : <Copy className="h-4 w-4" aria-hidden="true" />}
          <span aria-live="polite">{copied === 'invoice' ? 'Copied' : 'Copy invoice'}</span>
        </button>
        <a href={`lightning:${invoice.payment_request}`} className="btn btn-quiet btn-block">
          <ExternalLink className="h-4 w-4" aria-hidden="true" />
          Open in wallet
        </a>
        {payment.lnurl && (
          <button type="button" onClick={() => copy(payment.lnurl ?? '', 'lnurl')} className="btn btn-ghost">
            {copied === 'lnurl' ? 'Copied' : 'Copy LNURL instead'}
          </button>
        )}
      </div>
      <p className="label">Keep this page open. We will confirm as soon as your payment arrives.</p>
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
            ? `Sending to ${payment.recipient_name ?? 'the recipient'}. Bank transfers can take a while depending on the bank's hours. This is normal.`
            : `Converting to ${payment.destination_currency} now.`}
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
            The payout partner reversed this payment after it had been reported as delivered. Add a Lightning invoice below to receive your refund.
          </Notice>
        );
      }
      return (
        <Notice tone="pending" pill={<StatusPill tone="pending" label="Refund needed" />}>
          {payment.failure_reason ?? 'This payment could not be completed.'} Add a Lightning invoice below to receive your refund.
        </Notice>
      );
    case 'REFUNDED':
      return (
        <Notice tone="refunded" pill={<StatusPill tone="refunded" label="Refunded" />}>
          Your refund was sent{payment.refunded_at ? ` on ${new Date(payment.refunded_at).toLocaleString()}` : ''}.
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
    <Notice tone="pending" pill={<StatusPill tone="pending" label="Invoice expired" />}>
      No payment arrived in time, so nothing was sent. Start a new quote to try again with the same amount.
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

function RefundForm({ payment }: { payment: PaymentDetails }) {
  const queryClient = useQueryClient();
  const [invoice, setInvoice] = useState('');
  const [localError, setLocalError] = useState<string | null>(null);
  const refund = useMutation({
    mutationFn: (paymentRequest: string) => refundPayment(payment.id, paymentRequest),
    onSuccess: () => {
      setInvoice('');
      return queryClient.invalidateQueries({ queryKey: ['payment', payment.id] });
    },
  });

  const submit = () => {
    const trimmed = invoice.trim();
    if (!trimmed.toLowerCase().startsWith('ln')) {
      setLocalError('Paste a Lightning invoice, which starts with “ln”.');
      return;
    }
    setLocalError(null);
    refund.mutate(trimmed);
  };

  const serverError = refund.error as ApiError | null;

  return (
    <section aria-labelledby="refund" className="card flex flex-col gap-3">
      <h2 id="refund" className="text-lg font-semibold">
        Get your refund
      </h2>
      <p className="label">
        Paste a Lightning invoice for exactly <strong className="money text-[var(--text)]">{payment.amount_sats.toLocaleString('en-US')} sats</strong>. The
        invoice amount must match, or the refund will be rejected.
      </p>
      <label htmlFor="refund-invoice" className="label">
        Refund invoice
      </label>
      <textarea
        id="refund-invoice"
        rows={3}
        value={invoice}
        onChange={(e) => setInvoice(e.target.value)}
        className="field font-mono text-xs"
        style={{ borderRadius: 14, padding: 12 }}
        placeholder="lnbc…"
        spellCheck={false}
        autoComplete="off"
      />
      {localError && (
        <p role="alert" className="error-text">
          {localError}
        </p>
      )}
      {serverError && (
        <p role="alert" className="error-text">
          {serverError.code === 'validation_error' && serverError.detail ? serverError.detail : refund.error?.message}
        </p>
      )}
      <button type="button" onClick={submit} disabled={invoice.trim() === '' || refund.isPending} className="btn btn-accent btn-block">
        {refund.isPending ? 'Sending refund…' : 'Send refund'}
      </button>
    </section>
  );
}

function Refunded({ payment }: { payment: PaymentDetails }) {
  return (
    <section aria-labelledby="refunded" className="card flex flex-col gap-2">
      <h2 id="refunded" className="text-lg font-semibold">
        Refund details
      </h2>
      <p className="label">
        Refunded {payment.refund_sats?.toLocaleString('en-US') ?? payment.amount_sats.toLocaleString('en-US')} sats
        {payment.refunded_at ? ` on ${new Date(payment.refunded_at).toLocaleString()}` : ''}.
      </p>
    </section>
  );
}

function Receipt({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const payout = payment.payout;
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
        <Row label="Fee" value={`${payment.fee_sats.toLocaleString('en-US')} sats`} />
        <Row label="Rate" value={`1 ${payment.source_currency} ≈ ${payment.exchange_rate} ${payment.destination_currency}`} />
        <Row label="Recipient" value={payment.recipient_name ?? 'Not checked'} />
        <Row label="Account" value={payment.recipient_masked} />
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

function DevPayoutPanel({ payment }: { payment: PaymentDetails }) {
  const queryClient = useQueryClient();
  const simulate = useMutation({
    mutationFn: (status: 'success' | 'failed' | 'reversed') => simulatePayout(payment.id, status),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['payment', payment.id] }),
  });
  const enabled = process.env.NEXT_PUBLIC_DEV_TOOLS === 'true' && payment.payout?.provider.endsWith('-mock') === true;
  if (!enabled || payment.payout?.status !== 'submitted') return null;
  return (
    <section aria-label="Developer tools" className="card flex flex-col gap-3 border-dashed text-sm">
      <p className="font-semibold">Dev: simulate payout</p>
      <p className="label">Mock provider only. Not shown in production builds.</p>
      <div className="flex flex-wrap gap-2">
        <button type="button" onClick={() => simulate.mutate('success')} disabled={simulate.isPending} className="btn btn-quiet">
          Success
        </button>
        <button type="button" onClick={() => simulate.mutate('failed')} disabled={simulate.isPending} className="btn btn-quiet">
          Failed
        </button>
        <button type="button" onClick={() => simulate.mutate('reversed')} disabled={simulate.isPending} className="btn btn-quiet">
          Reversed
        </button>
      </div>
      {simulate.isError && <p role="alert" className="error-text">{simulate.error.message}</p>}
    </section>
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
