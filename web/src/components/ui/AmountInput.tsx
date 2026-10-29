'use client';

import { useId, type Ref } from 'react';

interface AmountInputProps {
  label: string;
  value: string;
  onChange: (text: string) => void;
  disabled?: boolean;
  variant: 'hero' | 'card';
  inputRef?: Ref<HTMLInputElement>;
}

export function AmountInput({ label, value, onChange, disabled, variant, inputRef }: AmountInputProps) {
  const id = useId();
  return (
    <>
      <label className="sr-only" htmlFor={id}>
        {label}
      </label>
      <input
        ref={inputRef}
        id={id}
        type="text"
        inputMode="decimal"
        autoComplete="off"
        placeholder="0"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        className={variant === 'hero' ? 'amount-hero' : 'amount-card-input'}
      />
    </>
  );
}
