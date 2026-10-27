'use client';

import { useId } from 'react';

interface MoneyFieldProps {
  label: string;
  code: string;
  value: string;
  onChange: (text: string) => void;
  disabled?: boolean;
  readOnly?: boolean;
  hint?: string;
  error?: string;
  helper?: React.ReactNode;
}

export function MoneyField({ label, code, value, onChange, disabled, readOnly, hint, error, helper }: MoneyFieldProps) {
  const id = useId();
  return (
    <div className="space-y-1">
      <label htmlFor={id} className="block text-sm font-medium">
        {label}
      </label>
      <div className="flex items-center gap-2">
        <input
          id={id}
          type="text"
          inputMode="decimal"
          autoComplete="off"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={disabled}
          readOnly={readOnly}
          placeholder="0"
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${id}-error` : hint ? `${id}-hint` : undefined}
          className="form-input text-2xl font-semibold"
        />
        <span className="shrink-0 font-semibold text-muted">{code}</span>
      </div>
      {helper}
      {hint && !error && (
        <p id={`${id}-hint`} className="text-xs text-muted">
          {hint}
        </p>
      )}
      {error && (
        <p id={`${id}-error`} className="text-sm text-error-700 dark:text-error-300" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
