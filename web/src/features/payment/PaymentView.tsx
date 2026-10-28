'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { AlertCircle, CheckCircle2, Clock, Copy, ExternalLink } from 'lucide-react';
import { LightningQr } from '@/components/QrCode';
import { useCountdown } from '@/hooks/useCountdown';
import { useCurrencies } from '@/hooks/useCurrencies';
import { usePaymentDetails } from '@/hooks/usePayment';
import { payPayment, refundPayment, simulatePayout } from '@/lib/api';
import type { ApiError } from '@/lib/api/errors';
import type { CurrencyInfo, PaymentDetails, PaymentStatus } from '@/lib/api/schemas';
import { formatWithCode } from '@/lib/money';
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
      <p role="alert" className="rounded-lg bg-error-50 px-3 py-2 text-error-700 dark:bg-error-950 dark:text-error-200">
        {error.message}
      </p>
    );
  }
  const payment = query.data;
  const list = currencies.data;
  if (!payment || !list) return <p className="text-muted">Loading payment…</p>;
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
        <p role="alert" className="rounded-lg bg-error-50 px-3 py-2 text-error-700 dark:bg-error-950 dark:text-error-200">
          {invoice.error.message}
        </p>
      );
    }
    return <p className="text-muted">Preparing your Lightning invoice…</p>;
  }

  return (
    <div className="space-y-6">
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

  const copy = async (text: string, which: 'invoice' | 'lnurl') => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      setTimeout(() => setCopied(null), 2000);
    } catch {
      setCopied(null);
    }
  };

  if (isExpired) {
    return <Expired payment={payment} />;
  }

  return (
    <div className="space-y-6">
      <h1 className="text-center text-2xl font-bold">
        Pay {formatWithCode(payment.source_amount, expOf(currencies, payment.source_currency), payment.source_currency)}
      </h1>
      <div className="card card-content">
        <LightningQr value={invoice.payment_request} />
      </div>
      <p className="text-center text-3xl font-bold tabular-nums">
        {invoice.amount_sats.toLocaleString('en-US')} <span className="text-lg font-semibold">sats</span>
      </p>
      <p className="text-center text-sm text-muted">Scan with any Lightning wallet, or use the buttons below.</p>
      <div aria-live="polite" className="rounded-lg bg-neutral-100 px-3 py-2 text-center text-sm dark:bg-neutral-800">
        {secondsLeft <= 60 ? `Invoice expires in ${secondsLeft}s` : `Invoice valid for ${Math.ceil(secondsLeft / 60)} min`}
      </div>
      <div className="grid gap-2">
        <a href={`lightning:${invoice.payment_request}`} className="btn btn-primary gap-2">
          <ExternalLink className="h-4 w-4" aria-hidden="true" />
          Open in wallet
        </a>
        <button type="button" onClick={() => copy(invoice.payment_request, 'invoice')} className="btn btn-secondary gap-2">
          <Copy className="h-4 w-4" aria-hidden="true" />
          {copied === 'invoice' ? 'Copied' : 'Copy invoice'}
        </button>
        {payment.lnurl && (
          <button type="button" onClick={() => copy(payment.lnurl ?? '', 'lnurl')} className="btn btn-ghost text-sm">
            {copied === 'lnurl' ? 'Copied' : 'Copy LNURL instead'}
          </button>
        )}
      </div>
      <p className="text-xs text-muted">Keep this page open. We will show a confirmation as soon as your payment arrives.</p>
    </div>
  );
}

function Outcome({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  switch (payment.status) {
    case 'AWAITING_PAYMENT':
      return null;
    case 'PAYMENT_DETECTED':
    case 'SETTLED':
    case 'PAYOUT_PENDING':
      return (
        <Banner tone="info" icon={<Clock className="h-5 w-5" />} title="Payment received">
          {payment.status === 'PAYOUT_PENDING'
            ? `Sending to ${payment.recipient_name ?? 'the recipient'}. Bank transfers can take a while depending on the bank's hours. This is normal.`
            : `Converting to ${payment.destination_currency} now.`}
          {payment.payout?.needs_review && (
            <span className="mt-2 block">
              We are reviewing this payment before it is delivered. {payment.payout.review_reason ?? ''}
            </span>
          )}
        </Banner>
      );
    case 'COMPLETED':
      return (
        <Banner tone="success" icon={<CheckCircle2 className="h-6 w-6" />} title="Money delivered">
          {payment.recipient_name ?? 'The recipient'} has been paid{' '}
          {formatWithCode(payment.expected_payout, expOf(currencies, payment.destination_currency), payment.destination_currency)}.
        </Banner>
      );
    case 'EXPIRED':
      return <Expired payment={payment} />;
    case 'FAILED':
      if (isReversal(payment)) {
        return (
          <Banner tone="error" icon={<AlertCircle className="h-5 w-5" />} title="Payment reversed">
            The payout partner reversed this payment after it had been reported as delivered. No money is lost. Your refund will be prepared next.
          </Banner>
        );
      }
      return (
        <Banner tone="error" icon={<AlertCircle className="h-5 w-5" />} title="Payment failed">
          {payment.failure_reason ?? 'The payment could not be completed.'} Your refund will be prepared next.
        </Banner>
      );
    case 'REFUND_PENDING':
      if (payment.payout?.status === 'reversed') {
        return (
          <Banner tone="error" icon={<AlertCircle className="h-5 w-5" />} title="Payment reversed">
            The payout partner reversed this payment after it had been reported as delivered. Add a Lightning invoice below to receive your refund.
          </Banner>
        );
      }
      return (
        <Banner tone="warning" icon={<AlertCircle className="h-5 w-5" />} title="Refund needed">
          {payment.failure_reason ?? 'This payment could not be completed.'} Add a Lightning invoice below to receive your refund.
        </Banner>
      );
    case 'REFUNDED':
      return (
        <Banner tone="success" icon={<CheckCircle2 className="h-5 w-5" />} title="Refunded">
          Your refund was sent{payment.refunded_at ? ` on ${new Date(payment.refunded_at).toLocaleString()}` : ''}.
        </Banner>
      );
    default:
      return null;
  }
}

function Expired({ payment }: { payment: PaymentDetails }) {
  return (
    <Banner tone="warning" icon={<Clock className="h-5 w-5" />} title="Invoice expired">
      No payment arrived in time, so nothing was sent. Start a new quote to try again with the same amount.
      <Link
        href={`/?from=${encodeURIComponent(payment.id)}`}
        className="btn btn-primary mt-3 w-full"
      >
        Start a new quote
      </Link>
    </Banner>
  );
}

function Tracker({ payment }: { payment: PaymentDetails }) {
  const active = activeStepIndex(payment.status) ?? -1;
  return (
    <section aria-labelledby="tracker" className="card card-content">
      <h2 id="tracker" className="mb-4 text-base font-semibold">
        Progress
      </h2>
      <ol className="space-y-4">
        {TRACKER_STEPS.map((step, index) => {
          const done = index < active || payment.status === 'COMPLETED';
          const current = index === active && payment.status !== 'COMPLETED';
          return (
            <li key={step} className="flex items-center gap-3" aria-current={current ? 'step' : undefined}>
              <span
                className={
                  done
                    ? 'flex h-7 w-7 items-center justify-center rounded-full bg-success-600 text-white'
                    : current
                      ? 'flex h-7 w-7 items-center justify-center rounded-full bg-primary-600 text-white'
                      : 'flex h-7 w-7 items-center justify-center rounded-full bg-neutral-200 text-neutral-700 dark:bg-neutral-700 dark:text-neutral-200'
                }
                aria-hidden="true"
              >
                {done ? '✓' : index + 1}
              </span>
              <span className={current || done ? 'font-medium' : 'text-muted'}>
                {TRACKER_LABELS[index]}
                {done ? ' (done)' : current ? ' (in progress)' : ''}
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
    <section aria-labelledby="refund" className="card card-content space-y-3">
      <h2 id="refund" className="font-semibold">
        Get your refund
      </h2>
      <p className="text-sm text-muted">
        Paste a Lightning invoice for exactly <strong className="tabular-nums">{payment.amount_sats.toLocaleString('en-US')} sats</strong>. The
        invoice amount must match, or the refund will be rejected.
      </p>
      <label htmlFor="refund-invoice" className="block text-sm font-medium">
        Refund invoice
      </label>
      <textarea
        id="refund-invoice"
        rows={3}
        value={invoice}
        onChange={(e) => setInvoice(e.target.value)}
        className="form-input font-mono text-xs"
        placeholder="lnbc…"
        spellCheck={false}
        autoComplete="off"
      />
      {localError && <p role="alert" className="text-sm text-error-700 dark:text-error-300">{localError}</p>}
      {serverError && (
        <p role="alert" className="text-sm text-error-700 dark:text-error-300">
          {serverError.code === 'validation_error' && serverError.detail ? serverError.detail : refund.error?.message}
        </p>
      )}
      <button type="button" onClick={submit} disabled={invoice.trim() === '' || refund.isPending} className="btn btn-primary w-full">
        {refund.isPending ? 'Sending refund…' : 'Send refund'}
      </button>
    </section>
  );
}

function Refunded({ payment }: { payment: PaymentDetails }) {
  return (
    <section aria-labelledby="refunded" className="card card-content space-y-2 text-sm">
      <h2 id="refunded" className="font-semibold">
        Refund details
      </h2>
      <p>
        Refunded {payment.refund_sats?.toLocaleString('en-US') ?? payment.amount_sats.toLocaleString('en-US')} sats
        {payment.refunded_at ? ` on ${new Date(payment.refunded_at).toLocaleString()}` : ''}.
      </p>
    </section>
  );
}

function Receipt({ payment, currencies }: { payment: PaymentDetails; currencies: CurrencyInfo[] }) {
  const payout = payment.payout;
  return (
    <section aria-labelledby="receipt" className="card card-content space-y-4">
      <div className="flex items-center justify-between gap-2">
        <h2 id="receipt" className="font-semibold">
          Receipt
        </h2>
        <button type="button" onClick={() => window.print()} className="btn btn-ghost text-sm">
          Download receipt
        </button>
      </div>
      <dl className="grid gap-2 text-sm">
        <Row
          label="You sent"
          value={formatWithCode(payment.source_amount, expOf(currencies, payment.source_currency), payment.source_currency)}
        />
        <Row label="Fee" value={`${payment.fee_sats.toLocaleString('en-US')} sats`} />
        <Row label="Rate" value={`1 ${payment.source_currency} ≈ ${payment.exchange_rate} ${payment.destination_currency}`} />
        <Row
          label="They received"
          value={formatWithCode(payment.expected_payout, expOf(currencies, payment.destination_currency), payment.destination_currency)}
        />
        <Row label="Recipient" value={payment.recipient_name ?? 'Not checked'} />
        <Row label="Account" value={payment.recipient_masked} />
        <Row label="Method" value={RAIL_LABELS[payment.recipient_rail]} />
        {payout?.provider_reference && <Row label="Reference" value={payout.provider_reference} />}
        <Row label="Created" value={new Date(payment.created_at).toLocaleString()} />
        {payout?.completed_at && <Row label="Delivered" value={new Date(payout.completed_at).toLocaleString()} />}
      </dl>
      <div className="flex flex-col gap-2 pt-2 sm:flex-row">
        <Link href="/" className="btn btn-primary flex-1">
          Send another
        </Link>
      </div>
    </section>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-4">
      <dt className="text-muted">{label}</dt>
      <dd className="text-right font-medium tabular-nums">{value}</dd>
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
    <section aria-label="Developer tools" className="rounded-lg border border-dashed border-warning-600 p-4 text-sm space-y-2">
      <p className="font-semibold">Dev: simulate payout</p>
      <p className="text-muted">Mock provider only. Not shown in production builds.</p>
      <div className="flex flex-wrap gap-2">
        <button type="button" onClick={() => simulate.mutate('success')} disabled={simulate.isPending} className="btn btn-secondary">Success</button>
        <button type="button" onClick={() => simulate.mutate('failed')} disabled={simulate.isPending} className="btn btn-secondary">Failed</button>
        <button type="button" onClick={() => simulate.mutate('reversed')} disabled={simulate.isPending} className="btn btn-secondary">Reversed</button>
      </div>
      {simulate.isError && <p role="alert" className="text-error-700 dark:text-error-300">{simulate.error.message}</p>}
    </section>
  );
}

function Details({ payment }: { payment: PaymentDetails }) {
  return (
    <details className="card card-content text-sm">
      <summary className="cursor-pointer font-medium">Details</summary>
      <p className="mt-2 text-muted">
        Status: <span className="font-mono">{payment.status}</span>
        {payment.payout ? ` · payout ${payment.payout.status}` : ''}
      </p>
      <ol className="mt-3 space-y-2">
        {payment.events.map((event) => (
          <li key={event.id} className="border-l-2 border-[var(--card-border)] pl-3">
            <p className="font-medium">{event.event_type.replace(/\./g, ' ')}</p>
            <p className="text-xs text-muted">
              {new Date(event.created_at).toLocaleString()}
              {event.to_status ? ` · ${event.to_status}` : ''}
            </p>
          </li>
        ))}
      </ol>
    </details>
  );
}

function Banner({
  tone,
  icon,
  title,
  children,
}: {
  tone: 'info' | 'success' | 'warning' | 'error';
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  const palette = {
    info: 'bg-primary-50 text-primary-900 dark:bg-primary-950 dark:text-primary-100',
    success: 'bg-success-50 text-success-900 dark:bg-success-950 dark:text-success-100',
    warning: 'bg-warning-50 text-warning-900 dark:bg-warning-950 dark:text-warning-100',
    error: 'bg-error-50 text-error-900 dark:bg-error-950 dark:text-error-100',
  }[tone];
  return (
    <section role="status" aria-live="polite" className={`space-y-2 rounded-lg p-4 ${palette}`}>
      <div className="flex items-center gap-2 font-semibold">
        <span aria-hidden="true">{icon}</span>
        {title}
      </div>
      <div className="text-sm">{children}</div>
    </section>
  );
}
