'use client';

import { useState } from 'react';
import type { RecipientDraft } from './types';

interface LightningAddressRecipientProps {
  onConfirm: (draft: RecipientDraft) => void;
}

const ADDRESS = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;

export function LightningAddressRecipient({ onConfirm }: LightningAddressRecipientProps) {
  const [address, setAddress] = useState('');
  const valid = ADDRESS.test(address.trim());

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <label htmlFor="ln-address" className="block text-sm font-medium">
          Lightning Address
        </label>
        <input
          id="ln-address"
          type="text"
          autoComplete="off"
          placeholder="name@wallet.example"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          className="form-input"
          aria-describedby="ln-hint"
        />
        <p id="ln-hint" className="text-xs text-muted">
          This is paid to the recipient&rsquo;s own wallet provider, which converts at its own rate. The amount shown is an estimate.
        </p>
      </div>
      <button
        type="button"
        disabled={!valid}
        onClick={() =>
          onConfirm({
            recipient: { rail: 'LIGHTNING_ADDRESS', address: address.trim().toLowerCase() },
            displayName: null,
            summary: address.trim().toLowerCase(),
          })
        }
        className="btn btn-primary w-full"
      >
        Use this address
      </button>
    </div>
  );
}
