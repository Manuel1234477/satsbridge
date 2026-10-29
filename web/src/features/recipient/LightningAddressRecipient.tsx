'use client';

import { useState } from 'react';
import { DEFAULT_ADDRESS_DOMAIN, normalizeLightningAddress } from '@/lib/lightning-address';
import type { RecipientDraft } from './types';

interface LightningAddressRecipientProps {
  onConfirm: (draft: RecipientDraft) => void;
}

export function LightningAddressRecipient({ onConfirm }: LightningAddressRecipientProps) {
  const [input, setInput] = useState('');
  const address = normalizeLightningAddress(input);

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-2">
        <label htmlFor="ln-address" className="label">
          Tapnob address
        </label>
        <input
          id="ln-address"
          type="text"
          inputMode="email"
          autoComplete="off"
          autoCapitalize="none"
          spellCheck={false}
          placeholder={`name@${DEFAULT_ADDRESS_DOMAIN}`}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          className="field"
          aria-describedby="ln-hint ln-preview"
        />
        <p id="ln-preview" className="label" aria-live="polite">
          {address ? (
            <>
              We will pay <strong className="text-[var(--text)]">{address}</strong>
            </>
          ) : input.trim() !== '' ? (
            'Enter a username or a full address like name@tapnob.io.'
          ) : (
            `Type the recipient's Tapnob username, or their full address.`
          )}
        </p>
        <p id="ln-hint" className="label">
          Tapnob pays naira into the recipient&rsquo;s linked bank account at its own rate, so the amount they get is an estimate.
          Their name is not checked, so make sure the address is right.
        </p>
      </div>
      <button
        type="button"
        disabled={address === null}
        onClick={() => address && onConfirm({ recipient: { rail: 'LIGHTNING_ADDRESS', address }, displayName: null, summary: address })}
        className="btn btn-accent btn-block"
      >
        Use this address
      </button>
    </div>
  );
}
