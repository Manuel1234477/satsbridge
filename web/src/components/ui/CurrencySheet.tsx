'use client';

import { useMemo, useState } from 'react';
import type { CurrencyInfo } from '@/lib/api/schemas';
import { Sheet } from './Sheet';
import { CurrencyBadge } from './CurrencyChip';

interface CurrencySheetProps {
  open: boolean;
  onClose: () => void;
  title: string;
  currencies: CurrencyInfo[];
  selected: string;
  onSelect: (code: string) => void;
  onlySendable?: boolean;
}

export function CurrencySheet({ open, onClose, title, currencies, selected, onSelect, onlySendable }: CurrencySheetProps) {
  const [query, setQuery] = useState('');
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return currencies.filter(
      (c) =>
        (!onlySendable || c.can_send) &&
        (q === '' || c.code.toLowerCase().includes(q) || c.name.toLowerCase().includes(q)),
    );
  }, [currencies, query, onlySendable]);

  return (
    <Sheet open={open} onClose={onClose} title={title}>
      <input
        type="search"
        className="field"
        placeholder="Search currency"
        aria-label="Search currency"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <div className="sheet-list" role="listbox" aria-label={title}>
        {visible.map((c) => (
          <button
            key={c.code}
            type="button"
            role="option"
            aria-selected={c.code === selected}
            className="sheet-option"
            onClick={() => {
              onSelect(c.code);
              onClose();
            }}
          >
            <CurrencyBadge code={c.code} />
            <span className="flex-1">
              <span className="block font-semibold">{c.code}</span>
              <span className="block text-sm text-[var(--text-2)]">{c.name}</span>
            </span>
          </button>
        ))}
        {visible.length === 0 && <p className="p-3 text-[var(--text-2)]">No currency matches “{query}”.</p>}
      </div>
    </Sheet>
  );
}
