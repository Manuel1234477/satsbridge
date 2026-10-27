'use client';

import { useState } from 'react';
import { Rail } from '@/lib/api/schemas';

interface RecipientSelectorProps {
  availableRails: Rail[];
  onRailSelect: (rail: Rail) => void;
}

export function RecipientSelector({ availableRails, onRailSelect }: RecipientSelectorProps) {
  const railLabels: Record<Rail, string> = {
    'KE_MOBILE_MONEY': 'Mobile Money (M-Pesa)',
    'NG_BANK_TRANSFER': 'Bank Transfer',
    'LIGHTNING_ADDRESS': 'Lightning Address',
    'MOBILE_MONEY': 'Mobile Money',
  };

  return (
    <div className="space-y-3">
      {availableRails.map((rail) => (
        <button
          key={rail}
          onClick={() => onRailSelect(rail)}
          className="w-full card p-4 text-left hover:bg-neutral-50 dark:hover:bg-neutral-800 transition-colors"
        >
          <div className="font-medium">{railLabels[rail]}</div>
        </button>
      ))}
    </div>
  );
}
