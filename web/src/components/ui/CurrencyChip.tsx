import { ChevronDown } from 'lucide-react';
import type { CurrencyInfo } from '@/lib/api/schemas';

export function CurrencyBadge({ code }: { code: string }) {
  return (
    <span className="badge" aria-hidden="true">
      {code.slice(0, 2)}
    </span>
  );
}

// Initials of the first two words, e.g. "United Bank for Africa" -> "UB".
export function Monogram({ name }: { name: string }) {
  const letters = name
    .split(/\s+/)
    .filter((w) => /[A-Za-z]/.test(w))
    .slice(0, 2)
    .map((w) => w.replace(/[^A-Za-z]/g, '')[0]?.toUpperCase() ?? '')
    .join('');
  return (
    <span className="badge" aria-hidden="true">
      {letters || name.slice(0, 2).toUpperCase()}
    </span>
  );
}

export function CurrencyChip({ code, onClick, disabled }: { code: string; onClick?: () => void; disabled?: boolean }) {
  return (
    <button type="button" className="chip" onClick={onClick} disabled={disabled} aria-haspopup="dialog" aria-label={`Currency ${code}. Change currency`}>
      <CurrencyBadge code={code} />
      <span>{code}</span>
      <ChevronDown className="h-4 w-4 text-[var(--text-2)]" aria-hidden="true" />
    </button>
  );
}

export function currencyName(currencies: CurrencyInfo[], code: string): string {
  return currencies.find((c) => c.code === code)?.name ?? code;
}
