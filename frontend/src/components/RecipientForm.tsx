import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { CurrencyCode, Institution, PayoutRail, Recipient, ResolveResponse } from '../lib/types';
import { ErrorNote } from './Bits';

export type Summary = [label: string, value: string][];

interface Props {
  currency: CurrencyCode;
  rail: PayoutRail;
  onSubmit: (recipient: Recipient, summary: Summary) => void;
}

export default function RecipientForm({ currency, rail, onSubmit }: Props) {
  switch (rail.rail) {
    case 'NG_BANK_TRANSFER':
      return <BankForm currency={currency} onSubmit={onSubmit} />;
    case 'LIGHTNING_ADDRESS':
      return <LightningAddressForm onSubmit={onSubmit} />;
    case 'KE_MOBILE_MONEY':
      return <PhoneForm onSubmit={onSubmit} />;
    case 'MOBILE_MONEY':
      return <MobileMoneyForm currency={currency} rail={rail} onSubmit={onSubmit} />;
  }
}

function useInstitutions(currency: CurrencyCode, rail: PayoutRail['rail'], country?: string, enabled = true) {
  const [list, setList] = useState<Institution[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    setList([]);
    setError(null);
    api
      .institutions(currency, rail, country)
      .then((l) => alive && setList(l))
      .catch((e: Error) => alive && setError(e.message));
    return () => {
      alive = false;
    };
  }, [currency, rail, country, enabled]);
  return { list, error };
}

function BankForm({ currency, onSubmit }: Pick<Props, 'currency' | 'onSubmit'>) {
  const { list: banks, error: banksError } = useInstitutions(currency, 'NG_BANK_TRANSFER');
  const [bank, setBank] = useState('');
  const [account, setAccount] = useState('');
  const [resolved, setResolved] = useState<ResolveResponse | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const valid = bank !== '' && /^\d{10}$/.test(account);

  // Any edit invalidates the verified name.
  useEffect(() => {
    setResolved(null);
    setConfirmed(false);
    setError(null);
  }, [bank, account]);

  async function verify() {
    setBusy(true);
    setError(null);
    try {
      setResolved(await api.resolve(currency, bank, account));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        if (!resolved || !confirmed) return;
        const bankName = banks.find((b) => b.code === bank)?.name ?? bank;
        onSubmit({ rail: 'NG_BANK_TRANSFER', bank_code: bank, account_number: account, resolution_id: resolved.resolution_id }, [
          ['Account name', resolved.account_name],
          ['Bank', bankName],
          ['Account', resolved.account_number_masked],
        ]);
      }}
    >
      <label className="field">
        <span>Bank</span>
        <select value={bank} onChange={(e) => setBank(e.target.value)} required>
          <option value="">Select a bank</option>
          {banks.map((b) => (
            <option key={b.code} value={b.code}>
              {b.name}
            </option>
          ))}
        </select>
      </label>
      {banksError && <ErrorNote>{banksError}</ErrorNote>}
      <label className="field">
        <span>Account number</span>
        <input
          inputMode="numeric"
          autoComplete="off"
          maxLength={10}
          placeholder="10 digits"
          value={account}
          onChange={(e) => setAccount(e.target.value.replace(/\D/g, ''))}
        />
      </label>
      {!resolved && (
        <button type="button" className="btn" disabled={!valid || busy} onClick={verify}>
          {busy ? 'Checking…' : 'Verify account'}
        </button>
      )}
      {error && <ErrorNote>{error}</ErrorNote>}
      {resolved && (
        <>
          <div className="verified">
            <span className="verified-label">Account holder</span>
            <strong>{resolved.account_name}</strong>
          </div>
          <label className="check">
            <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
            <span>This is the person I want to pay. Payouts cannot be undone.</span>
          </label>
          <button className="btn btn-primary" disabled={!confirmed}>
            Continue
          </button>
        </>
      )}
    </form>
  );
}

function LightningAddressForm({ onSubmit }: Pick<Props, 'onSubmit'>) {
  const [address, setAddress] = useState('');
  const valid = /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(address.trim());
  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        if (valid) onSubmit({ rail: 'LIGHTNING_ADDRESS', address: address.trim() }, [['Lightning Address', address.trim()]]);
      }}
    >
      <label className="field">
        <span>Lightning Address</span>
        <input
          autoComplete="off"
          autoCapitalize="none"
          placeholder="name@tapnob.io"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
        />
      </label>
      <p className="muted small">
        The recipient receives naira at their provider’s own rate, so the amount shown is an estimate. We can’t check the
        account holder’s name for this option; double-check the address.
      </p>
      <button className="btn btn-primary" disabled={!valid}>
        Continue
      </button>
    </form>
  );
}

function PhoneForm({ onSubmit }: Pick<Props, 'onSubmit'>) {
  const [phone, setPhone] = useState('');
  const valid = phone.replace(/\D/g, '').length >= 9;
  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        if (valid) onSubmit({ rail: 'KE_MOBILE_MONEY', phone_number: phone.trim() }, [['M-Pesa number', phone.trim()]]);
      }}
    >
      <label className="field">
        <span>M-Pesa phone number</span>
        <input inputMode="tel" autoComplete="off" placeholder="0712 345 678" value={phone} onChange={(e) => setPhone(e.target.value)} />
      </label>
      <button className="btn btn-primary" disabled={!valid}>
        Continue
      </button>
    </form>
  );
}

function MobileMoneyForm({ currency, rail, onSubmit }: Props) {
  const countries = rail.countries ?? [];
  const [country, setCountry] = useState(countries[0] ?? '');
  const { list: operators, error } = useInstitutions(currency, 'MOBILE_MONEY', country, country !== '');
  const [operator, setOperator] = useState('');
  const [phone, setPhone] = useState('');
  const [name, setName] = useState('');
  const valid = country !== '' && operator !== '' && phone.replace(/\D/g, '').length >= 8;

  useEffect(() => setOperator(''), [country]);

  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        if (!valid) return;
        const opName = operators.find((o) => o.code === operator)?.name ?? operator;
        onSubmit(
          {
            rail: 'MOBILE_MONEY',
            country,
            provider_code: operator,
            phone_number: phone.replace(/\D/g, ''),
            ...(name.trim() ? { account_name: name.trim() } : {}),
          },
          [
            ...(name.trim() ? ([['Name', name.trim()]] as Summary) : []),
            ['Operator', `${opName} (${country})`],
            ['Phone', phone.trim()],
          ],
        );
      }}
    >
      {countries.length > 0 ? (
        <label className="field">
          <span>Country</span>
          <select value={country} onChange={(e) => setCountry(e.target.value)}>
            {countries.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
      ) : (
        <label className="field">
          <span>Country code</span>
          <input maxLength={2} placeholder="SN" value={country} onChange={(e) => setCountry(e.target.value.toUpperCase())} />
        </label>
      )}
      <label className="field">
        <span>Mobile money operator</span>
        <select value={operator} onChange={(e) => setOperator(e.target.value)} required>
          <option value="">Select an operator</option>
          {operators.map((o) => (
            <option key={o.code} value={o.code}>
              {o.name}
            </option>
          ))}
        </select>
      </label>
      {error && <ErrorNote>{error}</ErrorNote>}
      <label className="field">
        <span>Phone number</span>
        <input inputMode="tel" autoComplete="off" value={phone} onChange={(e) => setPhone(e.target.value)} />
      </label>
      <label className="field">
        <span>Account holder name</span>
        <input autoComplete="off" placeholder="As registered with the operator" value={name} onChange={(e) => setName(e.target.value)} />
        <small className="muted">Some operators require this because the name can’t be looked up.</small>
      </label>
      <button className="btn btn-primary" disabled={!valid}>
        Continue
      </button>
    </form>
  );
}
