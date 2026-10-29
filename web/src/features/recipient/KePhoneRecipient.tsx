'use client';

import { useState } from 'react';
import type { RecipientDraft } from './types';
import { maskTail } from './types';

interface KePhoneRecipientProps {
  onConfirm: (draft: RecipientDraft) => void;
}

// Accepts 07XXXXXXXX / 01XXXXXXXX or +2547XXXXXXXX; sends the 10-digit local form.
function normalize(input: string): string | null {
  const digits = input.replace(/\D/g, '');
  if (/^0[17]\d{8}$/.test(digits)) return digits;
  if (/^254[17]\d{8}$/.test(digits)) return `0${digits.slice(3)}`;
  return null;
}

export function KePhoneRecipient({ onConfirm }: KePhoneRecipientProps) {
  const [phone, setPhone] = useState('');
  const normalized = normalize(phone);

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <label htmlFor="ke-phone" className="block text-sm font-medium">
          M-Pesa phone number
        </label>
        <input
          id="ke-phone"
          type="tel"
          inputMode="tel"
          autoComplete="off"
          placeholder="0712 345 678"
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
          className="form-input font-mono"
          aria-describedby="ke-hint"
        />
        <p id="ke-hint" className="text-xs text-muted">
          Kenyan number: 07 or 01 followed by 8 digits, or +254.
        </p>
      </div>
      <button
        type="button"
        disabled={normalized === null}
        onClick={() =>
          normalized &&
          onConfirm({
            recipient: { rail: 'KE_MOBILE_MONEY', phone_number: normalized },
            displayName: null,
            summary: `M-Pesa · ${maskTail(normalized)}`,
          })
        }
        className="btn btn-primary w-full"
      >
        Use this number
      </button>
    </div>
  );
}
