import type { CurrencyInfo } from './api/schemas';

// Currencies a recipient can be paid in: the backend lists at least one payout rail for them.
// A currency with none (e.g. GNF without a configured provider) can only be sent, never received.
export function receivableCurrencies(currencies: CurrencyInfo[]): CurrencyInfo[] {
  return currencies.filter((c) => c.payout_rails.length > 0);
}
