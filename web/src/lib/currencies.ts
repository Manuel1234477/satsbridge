import type { CurrencyInfo } from './api/schemas';

// Currencies a recipient can be paid in: the backend lists at least one payout rail for them.
export function receivableCurrencies(currencies: CurrencyInfo[]): CurrencyInfo[] {
  return currencies.filter((c) => c.payout_rails.length > 0);
}

// Currencies a sender can pay in: a bank collection rail exists (NGN today).
export function sendableCurrencies(currencies: CurrencyInfo[]): CurrencyInfo[] {
  return currencies.filter((c) => c.can_send);
}
