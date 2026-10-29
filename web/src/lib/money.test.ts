import { describe, it, expect } from 'vitest';
import { formatMoney, parseMoneyInput, getExponent } from './money';

describe('money', () => {
  describe('getExponent', () => {
    it('returns 0 for XOF', () => {
      expect(getExponent('XOF')).toBe(0);
    });

    it('returns 2 for NGN', () => {
      expect(getExponent('NGN')).toBe(2);
    });
  });

  describe('formatMoney', () => {
    it('formats XOF correctly (no decimals)', () => {
      expect(formatMoney(100000, 'XOF')).toBe('100,000');
    });

    it('formats NGN correctly (2 decimals)', () => {
      expect(formatMoney(10050, 'NGN')).toBe('100.50');
    });

    it('handles zero', () => {
      expect(formatMoney(0, 'NGN')).toBe('0.00');
    });
  });

  describe('parseMoneyInput', () => {
    it('parses input with exponent 0', () => {
      expect(parseMoneyInput('100', 0)).toBe(100);
    });

    it('parses input with exponent 2', () => {
      expect(parseMoneyInput('100.50', 2)).toBe(10050);
    });

    it('returns null for empty input', () => {
      expect(parseMoneyInput('', 2)).toBeNull();
    });

    it('returns null for invalid input', () => {
      expect(parseMoneyInput('abc', 2)).toBeNull();
    });
  });
});
