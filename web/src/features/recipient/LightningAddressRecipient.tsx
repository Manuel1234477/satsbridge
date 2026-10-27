'use client';

import { useState } from 'react';

interface LightningAddressRecipientProps {
  onSubmit: (address: string) => void;
}

export function LightningAddressRecipient({ onSubmit }: LightningAddressRecipientProps) {
  const [address, setAddress] = useState('');

  const isValid = address.includes('@') && address.split('@').length === 2;

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <label className="block text-sm font-medium">Lightning address</label>
        <input
          type="email"
          placeholder="name@example.com"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          className="form-input"
        />
        <p className="text-xs text-muted">
          The recipient receives their own provider's rate. Amount shown is an estimate.
        </p>
      </div>

      <button
        type="button"
        onClick={() => onSubmit(address)}
        disabled={!isValid}
        className="btn btn-primary w-full"
      >
        Continue with this address
      </button>
    </div>
  );
}
