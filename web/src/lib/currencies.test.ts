import { describe, expect, it } from 'vitest';
import type { CurrencyInfo } from './api/schemas';
import { receivableCurrencies } from './currencies';

const currency = (code: CurrencyInfo['code'], rails: CurrencyInfo['payout_rails']): CurrencyInfo => ({
  code,
  name: code,
  exponent: 0,
  payout_rails: rails,
});

describe('receivableCurrencies', () => {
  it('drops currencies with no payout rail, keeping the order of the rest', () => {
    const list = [
      currency('XOF', [{ rail: 'MOBILE_MONEY', provider: 'mock', min_payout_minor: null, max_payout_minor: null, guaranteed_amount: true }]),
      currency('GNF', []),
      currency('NGN', [{ rail: 'NG_BANK_TRANSFER', provider: 'bitnob', min_payout_minor: 50000, max_payout_minor: null, guaranteed_amount: true }]),
    ];
    expect(receivableCurrencies(list).map((c) => c.code)).toEqual(['XOF', 'NGN']);
  });
});
