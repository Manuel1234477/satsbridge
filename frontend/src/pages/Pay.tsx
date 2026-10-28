import { useCallback, useEffect, useRef, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { CopyButton, ErrorNote, Qr, StatusBadge } from '../components/Bits';
import { api } from '../lib/api';
import { formatClock, useCountdown, useCurrencies } from '../lib/hooks';
import { RAIL_LABEL, SETTLED_STATES, formatDateTime, humanizeEvent } from '../lib/labels';
import { formatAmount, formatSats } from '../lib/money';
import type { PaymentDetails, PaymentStatus } from '../lib/types';

const STAGES = ['Awaiting payment', 'Payment received', 'Sending payout', 'Delivered'];

function stage(status: PaymentStatus): number {
  switch (status) {
    case 'PAYMENT_DETECTED':
    case 'SETTLED':
      return 1;
    case 'PAYOUT_PENDING':
      return 2;
    case 'COMPLETED':
      return 3;
    default:
      return 0;
  }
}

export default function Pay() {
  const { id = '' } = useParams();
  const { currencies } = useCurrencies();
  const [details, setDetails] = useState<PaymentDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showLnurl, setShowLnurl] = useState(false);
  const requestedInvoice = useRef(false);
  const finished = useRef(false);

  const load = useCallback(async () => {
    try {
      const fresh = await api.payment(id);
      finished.current = SETTLED_STATES.includes(fresh.status);
      setDetails(fresh);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [id]);

  // Poll until the payment is in a state that needs nothing more from the server.
  useEffect(() => {
    load();
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible' && !finished.current) load();
    }, 3000);
    return () => clearInterval(timer);
  }, [load]);

  // The invoice is created on first request; asking again is safe.
  const status = details?.status;
  const hasInvoice = details?.lightning != null;
  useEffect(() => {
    if (!details || hasInvoice || requestedInvoice.current) return;
    if (status !== 'QUOTED' && status !== 'AWAITING_PAYMENT') return;
    requestedInvoice.current = true;
    api
      .pay(id)
      .then(load)
      .catch((e: Error) => {
        requestedInvoice.current = false;
        setError(e.message);
      });
  }, [details, hasInvoice, status, id, load]);

  const left = useCountdown(details?.lightning?.expires_at);
  const fmt = (n: number, c: PaymentDetails['source_currency']) => formatAmount(n, c, currencies);

  if (!details) {
    return error ? <ErrorNote>{error}</ErrorNote> : <p className="muted center">Loading…</p>;
  }

  const waiting = details.status === 'QUOTED' || details.status === 'AWAITING_PAYMENT';
  const invoice = details.lightning?.payment_request;
  const done = SETTLED_STATES.includes(details.status);
  const current = stage(details.status);
  const mockPayout = details.payout?.provider.endsWith('-mock') && details.status === 'PAYOUT_PENDING';

  return (
    <div className="stack-lg">
      <header className="row-between">
        <div>
          <h1>
            {fmt(details.source_amount, details.source_currency)} → {fmt(details.expected_payout, details.destination_currency)}
          </h1>
          <p className="muted">
            To {details.recipient_name ?? details.recipient_masked} · {RAIL_LABEL[details.recipient_rail]}
          </p>
        </div>
        <StatusBadge status={details.status} />
      </header>
      {error && <ErrorNote>{error}</ErrorNote>}

      {waiting && (
        <section className="card stack center">
          <h2>Pay with Lightning</h2>
          {invoice ? (
            <>
              <Qr
                value={showLnurl && details.lnurl ? `lightning:${details.lnurl}` : `lightning:${invoice.toUpperCase()}`}
                label="Lightning payment QR code"
              />
              <p>
                <strong>{formatSats(details.amount_sats)}</strong>
                <span className="muted"> · {left > 0 ? `expires in ${formatClock(left)}` : 'invoice expired'}</span>
              </p>
              <div className="actions">
                <a className="btn btn-primary" href={`lightning:${invoice}`}>
                  Open in wallet
                </a>
                <CopyButton text={showLnurl && details.lnurl ? details.lnurl : invoice} label="Copy invoice" />
              </div>
              {details.lnurl && (
                <button className="link" onClick={() => setShowLnurl((v) => !v)}>
                  {showLnurl ? 'Show the standard invoice' : 'Show as LNURL instead'}
                </button>
              )}
              <code className="invoice">{showLnurl && details.lnurl ? details.lnurl : invoice}</code>
              <p className="muted small">This page updates by itself once your payment arrives.</p>
            </>
          ) : (
            <p className="muted">Creating your invoice…</p>
          )}
        </section>
      )}

      {!waiting && details.status !== 'EXPIRED' && (
        <ol className="timeline-steps">
          {STAGES.map((label, i) => (
            <li key={label} className={details.status === 'FAILED' || details.status.startsWith('REFUND') ? '' : i < current ? 'done' : i === current ? (done ? 'done' : 'on') : ''}>
              {label}
            </li>
          ))}
        </ol>
      )}

      {details.status === 'COMPLETED' && (
        <section className="card stack center">
          <h2>Delivered ✓</h2>
          <p className="muted">
            {details.recipient_name ?? details.recipient_masked} received {fmt(details.payout?.destination_amount ?? details.expected_payout, details.destination_currency)}.
          </p>
          <Link className="btn btn-primary" to="/">
            Send another
          </Link>
        </section>
      )}

      {details.status === 'EXPIRED' && (
        <section className="card stack center">
          <h2>This payment expired</h2>
          <p className="muted">The invoice was not paid in time. Nothing was charged.</p>
          <Link className="btn btn-primary" to="/">
            Start again
          </Link>
        </section>
      )}

      {(details.status === 'REFUND_PENDING' || details.status === 'FAILED') && <RefundCard details={details} onDone={load} />}

      {details.status === 'REFUNDED' && (
        <section className="card stack">
          <h2>Refunded</h2>
          <p className="muted">
            {formatSats(details.refund_sats ?? details.amount_sats)} was sent back over Lightning
            {details.refunded_at ? ` on ${formatDateTime(details.refunded_at)}` : ''}.
          </p>
        </section>
      )}

      {import.meta.env.DEV && mockPayout && (
        <section className="card stack dev">
          <strong>Dev only</strong>
          <p className="muted small">This payout uses a mock provider ({details.payout?.provider}). Simulate its callback:</p>
          <div className="actions">
            <button className="btn" onClick={() => api.simulatePayout(id, 'success').then(load).catch((e: Error) => setError(e.message))}>
              Payout succeeds
            </button>
            <button className="btn" onClick={() => api.simulatePayout(id, 'failed').then(load).catch((e: Error) => setError(e.message))}>
              Payout fails
            </button>
          </div>
        </section>
      )}

      <section className="card stack">
        <h2>Receipt</h2>
        <dl className="rows">
          <Row label="Reference" value={details.id} mono />
          <Row label="Sent" value={fmt(details.source_amount, details.source_currency)} />
          <Row label="Fee" value={formatSats(details.fee_sats)} />
          <Row label="Recipient gets" value={fmt(details.expected_payout, details.destination_currency)} />
          <Row label="Recipient" value={`${details.recipient_name ?? ''} ${details.recipient_masked}`.trim()} />
          {details.payout?.provider_reference && <Row label="Provider reference" value={details.payout.provider_reference} mono />}
          {details.failure_reason && <Row label="Reason" value={details.failure_reason} />}
          <Row label="Created" value={formatDateTime(details.created_at)} />
        </dl>
        <h3>Timeline</h3>
        <ul className="events">
          {details.events.map((e) => (
            <li key={e.id}>
              <span>{humanizeEvent(e.event_type)}</span>
              <time className="muted small">{formatDateTime(e.created_at)}</time>
              {e.detail && <small className="muted">{e.detail}</small>}
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd className={mono ? 'mono' : ''}>{value}</dd>
    </div>
  );
}

function RefundCard({ details, onDone }: { details: PaymentDetails; onDone: () => void }) {
  const [invoice, setInvoice] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = details.refund_payment_hash !== null;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.refund(details.id, invoice.trim());
      onDone();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  }

  if (details.status === 'FAILED') {
    return (
      <section className="card stack">
        <h2>Something went wrong</h2>
        <p className="muted">{details.failure_reason ?? 'The payout could not be completed.'} Your refund is being prepared.</p>
      </section>
    );
  }
  return (
    <section className="card stack">
      <h2>Your payout didn’t go through</h2>
      <p className="muted">{details.failure_reason ?? 'The payout failed.'} You can get your {formatSats(details.amount_sats)} back.</p>
      {inFlight ? (
        <p className="note">Your refund is on its way. This page will update when it arrives.</p>
      ) : (
        <form className="stack" onSubmit={submit}>
          <label className="field">
            <span>Refund invoice</span>
            <textarea
              rows={4}
              placeholder="lnbc…"
              autoCapitalize="none"
              spellCheck={false}
              value={invoice}
              onChange={(e) => setInvoice(e.target.value)}
            />
            <small className="muted">
              Create an invoice in your wallet for exactly <strong>{details.amount_sats.toLocaleString()} sats</strong>, then paste it here.
            </small>
          </label>
          {error && <ErrorNote>{error}</ErrorNote>}
          <button className="btn btn-primary" disabled={busy || invoice.trim() === ''}>
            {busy ? 'Sending refund…' : 'Refund me'}
          </button>
        </form>
      )}
    </section>
  );
}
