import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ErrorNote } from '../components/Bits';
import RecipientForm, { type Summary } from '../components/RecipientForm';
import { ApiError, api } from '../lib/api';
import { formatClock, useCountdown, useCurrencies } from '../lib/hooks';
import { RAIL_HINT, RAIL_LABEL } from '../lib/labels';
import { formatAmount, formatSats, parseMinor } from '../lib/money';
import type { AmountSide, CurrencyCode, Quote, Rail, Recipient } from '../lib/types';

type Step = 'quote' | 'recipient' | 'review';

export default function Send() {
  const navigate = useNavigate();
  const { currencies, error, loading } = useCurrencies();
  const [source, setSource] = useState<CurrencyCode>('XOF');
  const [dest, setDest] = useState<CurrencyCode>('NGN');
  const [side, setSide] = useState<AmountSide>('source');
  const [amount, setAmount] = useState('');
  const [chosenRail, setChosenRail] = useState<Rail | null>(null);
  const [quote, setQuote] = useState<Quote | null>(null);
  const [quoting, setQuoting] = useState(false);
  const [quoteError, setQuoteError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [step, setStep] = useState<Step>('quote');
  const [recipient, setRecipient] = useState<{ recipient: Recipient; summary: Summary } | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const destInfo = currencies.find((c) => c.code === dest);
  const rails = destInfo?.payout_rails ?? [];
  const rail = rails.find((r) => r.rail === chosenRail) ?? rails[0];
  const amountCurrency = currencies.find((c) => c.code === (side === 'source' ? source : dest));
  const minor = amountCurrency ? parseMinor(amount, amountCurrency.exponent) : null;
  const left = useCountdown(quote?.expires_at);
  const expired = quote !== null && left === 0;
  const railCode = rail?.rail;

  // Quote automatically once the inputs are valid and typing has paused.
  useEffect(() => {
    setQuote(null);
    setQuoteError(null);
    if (!railCode || minor === null || source === dest) return;
    const ctl = new AbortController();
    const timer = setTimeout(async () => {
      setQuoting(true);
      try {
        setQuote(
          await api.createQuote(
            { source_currency: source, destination_currency: dest, amount: minor, amount_side: side, rail: railCode },
            ctl.signal,
          ),
        );
      } catch (e) {
        if (!ctl.signal.aborted) setQuoteError((e as Error).message);
      } finally {
        if (!ctl.signal.aborted) setQuoting(false);
      }
    }, 500);
    return () => {
      clearTimeout(timer);
      ctl.abort();
      setQuoting(false);
    };
  }, [source, dest, side, minor, railCode, refresh]);

  function swap() {
    setSource(dest);
    setDest(source);
    setChosenRail(null);
    setAmount('');
  }

  async function createPayment() {
    if (!quote || !recipient) return;
    setCreating(true);
    setCreateError(null);
    try {
      const payment = await api.createPayment(quote.id, recipient.recipient);
      navigate(`/pay/${payment.id}`);
    } catch (e) {
      setCreateError(e instanceof ApiError && e.status === 410 ? 'This quote expired. Go back and get a new one.' : (e as Error).message);
      setCreating(false);
    }
  }

  if (loading) return <p className="muted center">Loading…</p>;
  if (error) return <ErrorNote>{error}</ErrorNote>;

  const stepNo = step === 'quote' ? 1 : step === 'recipient' ? 2 : 3;
  const fmt = (n: number, c: CurrencyCode) => formatAmount(n, c, currencies);

  return (
    <div className="stack-lg">
      <header className="hero">
        <h1>Send money across Africa</h1>
        <p className="muted">Pay in your currency. They receive theirs, settled in seconds over Bitcoin Lightning.</p>
      </header>

      <ol className="steps" aria-label="Progress">
        {['Amount', 'Recipient', 'Review'].map((label, i) => (
          <li key={label} className={i + 1 === stepNo ? 'on' : i + 1 < stepNo ? 'done' : ''}>
            <span>{i + 1}</span> {label}
          </li>
        ))}
      </ol>

      {step === 'quote' && (
        <section className="card stack">
          <div className="seg" role="group" aria-label="Amount side">
            <button type="button" className={side === 'source' ? 'on' : ''} onClick={() => setSide('source')}>
              You send
            </button>
            <button type="button" className={side === 'destination' ? 'on' : ''} onClick={() => setSide('destination')}>
              They receive
            </button>
          </div>

          <div className="pair">
            <label className="field">
              <span>From</span>
              <select value={source} onChange={(e) => setSource(e.target.value as CurrencyCode)}>
                {currencies.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} · {c.name}
                  </option>
                ))}
              </select>
            </label>
            <button type="button" className="swap" onClick={swap} aria-label="Swap currencies">
              ⇄
            </button>
            <label className="field">
              <span>To</span>
              <select
                value={dest}
                onChange={(e) => {
                  setDest(e.target.value as CurrencyCode);
                  setChosenRail(null);
                }}
              >
                {currencies.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} · {c.name}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <label className="field">
            <span>{side === 'source' ? `Amount you send (${source})` : `Amount they receive (${dest})`}</span>
            <input
              className="amount"
              inputMode="decimal"
              autoComplete="off"
              placeholder="0"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </label>
          {amount !== '' && minor === null && amountCurrency && (
            <ErrorNote>
              Enter a positive amount with at most {amountCurrency.exponent} decimal place{amountCurrency.exponent === 1 ? '' : 's'}.
            </ErrorNote>
          )}
          {source === dest && <ErrorNote>Choose two different currencies.</ErrorNote>}

          {rails.length > 1 && (
            <fieldset className="rails">
              <legend>Deliver to</legend>
              {rails.map((r) => (
                <label key={r.rail} className={`railopt${r.rail === rail?.rail ? ' on' : ''}`}>
                  <input type="radio" name="rail" checked={r.rail === rail?.rail} onChange={() => setChosenRail(r.rail)} />
                  <span>
                    <strong>{RAIL_LABEL[r.rail]}</strong>
                    <small>
                      {RAIL_HINT[r.rail]}
                      {!r.guaranteed_amount && ' · estimated amount'}
                    </small>
                  </span>
                </label>
              ))}
            </fieldset>
          )}
          {rails.length === 0 && destInfo && <ErrorNote>No payout option is available for {dest} right now.</ErrorNote>}

          {quoting && <p className="muted center">Getting your rate…</p>}
          {quoteError && <ErrorNote>{quoteError}</ErrorNote>}

          {quote && (
            <div className="quote">
              <dl className="rows">
                <div>
                  <dt>You send</dt>
                  <dd>{fmt(quote.source_amount, quote.source_currency)}</dd>
                </div>
                <div>
                  <dt>Fee</dt>
                  <dd>
                    {fmt(quote.fee_source_amount, quote.source_currency)} <small className="muted">({formatSats(quote.fee_sats)})</small>
                  </dd>
                </div>
                <div className="big">
                  <dt>{quote.payout_amount_guaranteed ? 'They receive' : 'They receive (estimate)'}</dt>
                  <dd>{fmt(quote.destination_amount, quote.destination_currency)}</dd>
                </div>
                <div>
                  <dt>Rate</dt>
                  <dd>
                    1 {quote.source_currency} = {Number(quote.exchange_rate).toLocaleString(undefined, { maximumSignificantDigits: 6 })}{' '}
                    {quote.destination_currency}
                  </dd>
                </div>
              </dl>
              <p className="muted small">
                {expired ? (
                  'This rate has expired.'
                ) : (
                  <>
                    Rate locked for {formatClock(left)} · {quote.rate_source}
                  </>
                )}
              </p>
              {expired ? (
                <button className="btn btn-primary" onClick={() => setRefresh((n) => n + 1)}>
                  Get a new rate
                </button>
              ) : (
                <button className="btn btn-primary" onClick={() => setStep('recipient')}>
                  Continue
                </button>
              )}
            </div>
          )}
        </section>
      )}

      {step === 'recipient' && quote && rail && (
        <section className="card stack">
          <button className="link" onClick={() => setStep('quote')}>
            ← Back
          </button>
          <h2>Who are you paying?</h2>
          <p className="muted">
            {fmt(quote.destination_amount, quote.destination_currency)} via {RAIL_LABEL[quote.destination_rail]}
          </p>
          <RecipientForm
            key={rail.rail}
            currency={dest}
            rail={rail}
            onSubmit={(r, summary) => {
              setRecipient({ recipient: r, summary });
              setStep('review');
            }}
          />
        </section>
      )}

      {step === 'review' && quote && recipient && (
        <section className="card stack">
          <button className="link" onClick={() => setStep('recipient')}>
            ← Back
          </button>
          <h2>Review and confirm</h2>
          <dl className="rows">
            <div>
              <dt>You send</dt>
              <dd>{fmt(quote.source_amount, quote.source_currency)}</dd>
            </div>
            <div>
              <dt>Fee</dt>
              <dd>{fmt(quote.fee_source_amount, quote.source_currency)}</dd>
            </div>
            <div className="big">
              <dt>{quote.payout_amount_guaranteed ? 'They receive' : 'They receive (estimate)'}</dt>
              <dd>{fmt(quote.destination_amount, quote.destination_currency)}</dd>
            </div>
            {recipient.summary.map(([label, value]) => (
              <div key={label}>
                <dt>{label}</dt>
                <dd>{value}</dd>
              </div>
            ))}
          </dl>
          {expired && <ErrorNote>Your rate expired. Go back to the amount step to get a new one.</ErrorNote>}
          {createError && <ErrorNote>{createError}</ErrorNote>}
          <button className="btn btn-primary" disabled={creating || expired} onClick={createPayment}>
            {creating ? 'Creating…' : 'Confirm and get invoice'}
          </button>
          <p className="muted small">You will pay with a Lightning invoice on the next screen. This is a sandbox demo, not a licensed money transfer service.</p>
        </section>
      )}
    </div>
  );
}
