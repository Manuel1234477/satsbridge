'use client';

import type { Rail } from '@/lib/api/schemas';
import { RAIL_LABELS } from '@/lib/rails';

interface RailSelectorProps {
  rails: Rail[];
  value: Rail | null;
  onChange?: (rail: Rail) => void;
  label?: string;
}

// Segmented control. Without onChange it is read-only: the other options show but cannot be picked.
export function RailSelector({ rails, value, onChange, label = 'Payout method' }: RailSelectorProps) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {rails.map((rail) => (
        <button
          key={rail}
          type="button"
          role="radio"
          aria-checked={rail === value}
          disabled={!onChange && rail !== value}
          onClick={() => onChange?.(rail)}
        >
          {RAIL_LABELS[rail]}
        </button>
      ))}
    </div>
  );
}
