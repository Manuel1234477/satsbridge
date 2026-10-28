import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { ErrorNote, StatusBadge } from '../components/Bits';
import { api } from '../lib/api';
import { useCurrencies } from '../lib/hooks';
import { formatDateTime } from '../lib/labels';
import { formatAmount } from '../lib/money';
import type { PaymentIntent } from '../lib/types';

export default function History() {
  const { currencies } = useCurrencies();
  const [items, setItems] = useState<PaymentIntent[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .transactions()
      .then(setItems)
      .catch((e: Error) => setError(e.message));
  }, []);

  return (
    <div className="stack-lg">
      <h1>Your payments</h1>
      <p className="muted small">History is tied to this browser. Clearing site data starts a new, empty account.</p>
      {error && <ErrorNote>{error}</ErrorNote>}
      {items === null && !error && <p className="muted center">Loading…</p>}
      {items?.length === 0 && (
        <section className="card stack center">
          <p className="muted">No payments yet.</p>
          <Link className="btn btn-primary" to="/">
            Send money
          </Link>
        </section>
      )}
      <ul className="history">
        {items?.map((p) => (
          <li key={p.id}>
            <Link to={`/pay/${p.id}`} className="card tx">
              <div>
                <strong>
                  {formatAmount(p.source_amount, p.source_currency, currencies)} → {formatAmount(p.expected_payout, p.destination_currency, currencies)}
                </strong>
                <small className="muted">
                  {p.recipient_name ?? p.recipient_masked} · {formatDateTime(p.created_at)}
                </small>
              </div>
              <StatusBadge status={p.status} />
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}
