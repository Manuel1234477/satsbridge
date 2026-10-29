import { describe, expect, it } from 'vitest';
import { formatMinor, formatWithCode, parseMinor, toInputText } from './money';

describe('formatMinor', () => {
  it('formats exponent 0 currencies without decimals', () => {
    expect(formatMinor(100000, 0)).toBe('100,000');
    expect(formatMinor(0, 0)).toBe('0');
  });

  it('formats exponent 2 currencies with two decimals', () => {
    expect(formatMinor(10050, 2)).toBe('100.50');
    expect(formatMinor(5, 2)).toBe('0.05');
    expect(formatMinor(123456789, 2)).toBe('1,234,567.89');
  });

  it('formats negative values', () => {
    expect(formatMinor(-1050, 2)).toBe('-10.50');
  });

  it('appends the currency code', () => {
    expect(formatWithCode(245000, 2, 'NGN')).toBe('2,450.00 NGN');
  });
});

describe('parseMinor', () => {
  it('parses whole amounts for exponent 0', () => {
    expect(parseMinor('1000', 0)).toBe(1000);
    expect(parseMinor('1,000', 0)).toBe(1000);
  });

  it('rejects decimals for exponent 0', () => {
    expect(parseMinor('10.5', 0)).toBeNull();
  });

  it('parses two-decimal amounts for exponent 2', () => {
    expect(parseMinor('10.50', 2)).toBe(1050);
    expect(parseMinor('10.5', 2)).toBe(1050);
    expect(parseMinor('10', 2)).toBe(1000);
  });

  it('rejects empty, malformed and over-precise input', () => {
    expect(parseMinor('', 2)).toBeNull();
    expect(parseMinor('abc', 2)).toBeNull();
    expect(parseMinor('1.234', 2)).toBeNull();
    expect(parseMinor('-5', 2)).toBeNull();
  });
});

describe('toInputText', () => {
  it('round-trips with parseMinor', () => {
    expect(toInputText(1050, 2)).toBe('10.50');
    expect(toInputText(5, 2)).toBe('0.05');
    expect(toInputText(2450, 0)).toBe('2450');
    expect(parseMinor(toInputText(1050, 2), 2)).toBe(1050);
  });
});
