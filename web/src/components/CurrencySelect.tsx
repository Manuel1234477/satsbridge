'use client';

import { useId } from 'react';
import type { CurrencyInfo } from '@/lib/api/schemas';

interface CurrencySelectProps {
  label: string;
  currencies: CurrencyInfo[];
  value: string;
  onChange: (code: string) => void;
  disabled?: boolean;
  onlySendable?: boolean;
}

export function CurrencySelect({ label, currencies, value, onChange, disabled, onlySendable }: CurrencySelectProps) {
  const id = useId();
  const options = onlySendable ? currencies.filter((c) => c.can_send) : currencies;
  return (
    <div className="space-y-1">
      <label htmlFor={id} className="block text-sm font-medium">
        {label}
      </label>
      <select id={id} value={value} onChange={(e) => onChange(e.target.value)} disabled={disabled} className="form-input">
        {options.map((c) => (
          <option key={c.code} value={c.code}>
            {c.code} · {c.name}
          </option>
        ))}
      </select>
    </div>
  );
}
