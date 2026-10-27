'use client';

import { CurrencyInfo } from '@/lib/api/schemas';

interface CurrencySelectorProps {
  currencies: CurrencyInfo[];
  value: string;
  onChange: (code: string) => void;
  disabled?: boolean;
  sendable?: boolean;
}

export function CurrencySelector({
  currencies,
  value,
  onChange,
  disabled = false,
  sendable = false,
}: CurrencySelectorProps) {
  const filtered = sendable ? currencies.filter(c => c.can_send) : currencies;

  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
      className="form-input"
    >
      <option value="">Select currency...</option>
      {filtered.map((c) => (
        <option key={c.code} value={c.code}>
          {c.code} - {c.name}
        </option>
      ))}
    </select>
  );
}

export function CurrencyBadge({ code, name }: { code: string; name?: string }) {
  return (
    <span className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-neutral-100 dark:bg-neutral-800 text-sm font-medium">
      <span className="text-base">{code}</span>
      {name && <span className="text-neutral-600 dark:text-neutral-400">{name}</span>}
    </span>
  );
}
