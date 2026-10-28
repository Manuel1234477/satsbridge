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
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-2">
        <label htmlFor="ln-address" className="label">
          Lightning Address
        </label>
        <input
          id="ln-address"
          type="text"
          autoComplete="off"
          placeholder="name@wallet.example"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          className="field"
          aria-describedby="ln-hint"
        />
        <p id="ln-hint" className="label">
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
        className="btn btn-accent btn-block"
      >
        Use this address
      </button>
    </div>
  );
}
