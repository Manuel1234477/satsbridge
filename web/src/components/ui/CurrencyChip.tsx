import type { CurrencyInfo } from '@/lib/api/schemas';

// Monogram badge: the first letter of the code and its last letter, so it stays readable without flags.
export function CurrencyBadge({ code }: { code: string }) {
  return (
    <span className="badge" aria-hidden="true">
      {code.slice(0, 2)}
    </span>
  );
}

export function CurrencyChip({ code, onClick, disabled }: { code: string; onClick?: () => void; disabled?: boolean }) {
  return (
    <button type="button" className="chip" onClick={onClick} disabled={disabled} aria-haspopup="dialog">
      <CurrencyBadge code={code} />
      <span>{code}</span>
      <span aria-hidden="true" className="text-[var(--text-2)]">
        ▾
      </span>
    </button>
  );
}

export function currencyName(currencies: CurrencyInfo[], code: string): string {
  return currencies.find((c) => c.code === code)?.name ?? code;
}
