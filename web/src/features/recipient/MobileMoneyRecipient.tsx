'use client';

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getPayoutInstitutions } from '@/lib/api';
import type { Currency } from '@/lib/api/schemas';
import type { RecipientDraft } from './types';
import { maskTail } from './types';

interface MobileMoneyRecipientProps {
  currency: Currency;
  countries: string[];
  onConfirm: (draft: RecipientDraft) => void;
}

function countryName(code: string): string {
  try {
    return new Intl.DisplayNames(['en'], { type: 'region' }).of(code) ?? code;
  } catch {
    return code;
  }
}

export function MobileMoneyRecipient({ currency, countries, onConfirm }: MobileMoneyRecipientProps) {
  const [country, setCountry] = useState(countries.length === 1 ? countries[0] : '');
  const [operator, setOperator] = useState('');
  const [phone, setPhone] = useState('');
  const [name, setName] = useState('');

  const operators = useQuery({
    queryKey: ['institutions', currency, 'MOBILE_MONEY'],
    queryFn: () => getPayoutInstitutions(currency, 'MOBILE_MONEY'),
    staleTime: 60 * 60 * 1000,
  });

  const digits = phone.replace(/\D/g, '');
  const phoneValid = /^\d{8,12}$/.test(digits);
  const operatorName = operators.data?.find((o) => o.code === operator)?.name ?? operator;
  const ready = country !== '' && operator !== '' && phoneValid;

  return (
    <div className="flex flex-col gap-5">
      {countries.length > 1 && (
        <div className="flex flex-col gap-2">
          <label htmlFor="mm-country" className="label">
            Recipient&apos;s country
          </label>
          <select id="mm-country" className="field" value={country} onChange={(e) => setCountry(e.target.value)}>
            <option value="">Select a country</option>
            {countries.map((c) => (
              <option key={c} value={c}>
                {countryName(c)}
              </option>
            ))}
          </select>
        </div>
      )}
      <div className="flex flex-col gap-2">
        <label htmlFor="mm-operator" className="label">
          Mobile money provider
        </label>
        <select id="mm-operator" className="field" value={operator} onChange={(e) => setOperator(e.target.value)} disabled={!operators.data}>
          <option value="">{operators.isError ? 'Could not load providers' : 'Select a provider'}</option>
          {operators.data?.map((o) => (
            <option key={o.code} value={o.code}>
              {o.name}
            </option>
          ))}
        </select>
      </div>
      <div className="flex flex-col gap-2">
        <label htmlFor="mm-phone" className="label">
          Phone number
        </label>
        <input
          id="mm-phone"
          type="tel"
          inputMode="tel"
          autoComplete="off"
          placeholder="771234567"
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
          className="field money text-[22px]"
          aria-describedby="mm-hint"
        />
        <p id="mm-hint" className="label">
          Digits only, without the +, 8 to 12 digits.
        </p>
      </div>
      <div className="flex flex-col gap-2">
        <label htmlFor="mm-name" className="label">
          Account holder name (optional)
        </label>
        <input id="mm-name" type="text" autoComplete="off" value={name} onChange={(e) => setName(e.target.value)} className="field" />
      </div>
      <button
        type="button"
        disabled={!ready}
        onClick={() =>
          onConfirm({
            recipient: {
              rail: 'MOBILE_MONEY',
              country,
              provider_code: operator,
              phone_number: digits,
              ...(name.trim() ? { account_name: name.trim() } : {}),
            },
            displayName: name.trim() || null,
            summary: `${operatorName} · ${countryName(country)} · ${maskTail(digits)}`,
          })
        }
        className="btn btn-accent btn-block"
      >
        Use this number
      </button>
    </div>
  );
}
