import { Currency } from './api/schemas';

const currencyExponents: Record<Currency, number> = {
  XOF: 0,
  XAF: 0,
  GNF: 0,
  KES: 2,
  NGN: 2,
};

export function getExponent(currency: Currency): number {
  return currencyExponents[currency] ?? 0;
}

export function formatMoney(amount: number, currency: Currency): string {
  const exponent = getExponent(currency);
  const formatter = new Intl.NumberFormat('en-US', {
    minimumFractionDigits: exponent,
    maximumFractionDigits: exponent,
    useGrouping: true,
  });
  const value = amount / Math.pow(10, exponent);
  return formatter.format(value);
}

export function formatMoneyWithCurrency(amount: number, currency: Currency): string {
  return `${formatMoney(amount, currency)} ${currency}`;
}

export function parseMoneyInput(input: string, exponent: number): number | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  const num = parseFloat(trimmed);
  if (isNaN(num)) return null;

  return Math.round(num * Math.pow(10, exponent));
}
