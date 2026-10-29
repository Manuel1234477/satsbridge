import type { CurrencyCode, CurrencyInfo } from './types';

/** Integer minor units to a display string, e.g. 12345 NGN -> "123.45". */
export function formatMinor(minor: number, exponent: number): string {
  return (minor / 10 ** exponent).toLocaleString(undefined, {
    minimumFractionDigits: exponent,
    maximumFractionDigits: exponent,
  });
}

export function formatAmount(minor: number, code: CurrencyCode, currencies: CurrencyInfo[]): string {
  const exponent = currencies.find((c) => c.code === code)?.exponent ?? 0;
  return `${formatMinor(minor, exponent)} ${code}`;
}

/** Parses what the user typed into minor units without floating point error. Null if invalid. */
export function parseMinor(input: string, exponent: number): number | null {
  const text = input.replace(/[,\s]/g, '');
  const match = /^(\d+)(?:\.(\d*))?$/.exec(text);
  if (!match) return null;
  const fraction = match[2] ?? '';
  if (fraction.length > exponent) return null;
  const minor = Number(match[1] + fraction.padEnd(exponent, '0'));
  return Number.isSafeInteger(minor) && minor > 0 ? minor : null;
}

export function formatSats(sats: number): string {
  return `${sats.toLocaleString()} sats`;
}
