'use client';

import { useCallback } from 'react';
import { parseMoneyInput, getExponent } from '@/lib/money';
import { Currency } from '@/lib/api/schemas';

interface MoneyInputProps {
  value: string;
  onChange: (minorUnits: number | null) => void;
  currency?: Currency;
  placeholder?: string;
  disabled?: boolean;
  readOnly?: boolean;
}

export function MoneyInput({
  value,
  onChange,
  currency,
  placeholder = '0',
  disabled = false,
  readOnly = false,
}: MoneyInputProps) {
  const exponent = currency ? getExponent(currency) : 0;

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const input = e.target.value;
      const parsed = parseMoneyInput(input, exponent);
      onChange(parsed);
    },
    [exponent, onChange]
  );

  return (
    <input
      type="text"
      inputMode="decimal"
      value={value}
      onChange={handleChange}
      placeholder={placeholder}
      disabled={disabled || readOnly}
      readOnly={readOnly}
      className="form-input text-money-md font-semibold"
    />
  );
}
