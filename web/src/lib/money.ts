// All amounts are integer minor units. The exponent comes from GET /currencies.
// Parsing and formatting use integer/string arithmetic only; no float math touches money.

export function formatMinor(minor: number, exponent: number): string {
  const negative = minor < 0;
  const digits = Math.abs(minor).toString().padStart(exponent + 1, '0');
  const whole = exponent === 0 ? digits : digits.slice(0, digits.length - exponent);
  const fraction = exponent === 0 ? '' : digits.slice(digits.length - exponent);
  const grouped = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(Number(whole));
  return `${negative ? '-' : ''}${grouped}${fraction ? `.${fraction}` : ''}`;
}

export function formatWithCode(minor: number, exponent: number, code: string): string {
  return `${formatMinor(minor, exponent)} ${code}`;
}

// Parses user input such as "1000", "1,000" or "10.50" into minor units.
// Returns null for empty, malformed, or over-precise input (more decimals than the currency has).
export function parseMinor(input: string, exponent: number): number | null {
  const cleaned = input.replace(/[\s,]/g, '').trim();
  if (cleaned === '') return null;
  const match = /^(\d+)(?:\.(\d+))?$/.exec(cleaned);
  if (!match) return null;
  const whole = match[1];
  const fraction = match[2] ?? '';
  if (fraction.length > exponent) return null;
  const padded = fraction.padEnd(exponent, '0');
  const minor = Number(`${whole}${padded}`);
  return Number.isSafeInteger(minor) ? minor : null;
}

// Editable text for a minor amount, e.g. 1050 at exponent 2 -> "10.50".
export function toInputText(minor: number, exponent: number): string {
  if (exponent === 0) return minor.toString();
  const digits = minor.toString().padStart(exponent + 1, '0');
  return `${digits.slice(0, digits.length - exponent)}.${digits.slice(digits.length - exponent)}`;
}
