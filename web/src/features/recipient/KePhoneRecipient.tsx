'use client';

import { useState } from 'react';

interface KePhoneRecipientProps {
  onSubmit: (phoneNumber: string) => void;
}

export function KePhoneRecipient({ onSubmit }: KePhoneRecipientProps) {
  const [phone, setPhone] = useState('');

  const formatPhone = (value: string): string => {
    const digits = value.replace(/\D/g, '');
    if (digits.startsWith('254')) return '+' + digits;
    if (digits.startsWith('0')) return '+254' + digits.slice(1);
    return '+254' + digits;
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const digits = e.target.value.replace(/\D/g, '').slice(0, 12);
    setPhone(digits);
  };

  const isValid = phone.replace(/\D/g, '').length === 10 || phone.replace(/\D/g, '').length === 12;
  const displayPhone = formatPhone(phone);

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <label className="block text-sm font-medium">Phone number</label>
        <input
          type="tel"
          placeholder="0712 345 678"
          value={phone}
          onChange={handleChange}
          className="form-input font-mono"
        />
        {phone && (
          <p className="text-xs text-muted">Formatted: {displayPhone}</p>
        )}
      </div>

      <button
        type="button"
        onClick={() => onSubmit(formatPhone(phone))}
        disabled={!isValid}
        className="btn btn-primary w-full"
      >
        Continue with this number
      </button>
    </div>
  );
}
