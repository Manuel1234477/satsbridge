import { describe, expect, it } from 'vitest';
import { isRateStale, rateAgeSeconds } from './rate-freshness';

const asOf = '2026-10-28T12:00:00.000Z';
const at = (seconds: number) => new Date(asOf).getTime() + seconds * 1000;

describe('rateAgeSeconds', () => {
  it('counts whole seconds since the rate was fetched', () => {
    expect(rateAgeSeconds(asOf, at(12))).toBe(12);
  });

  it('never goes negative when the client clock is behind', () => {
    expect(rateAgeSeconds(asOf, at(-5))).toBe(0);
  });
});

describe('isRateStale', () => {
  it('is live for a rate fetched within two minutes', () => {
    expect(isRateStale(asOf, at(119), false)).toBe(false);
    expect(isRateStale(asOf, at(120), false)).toBe(false);
  });

  it('is stale once the rate is older than two minutes', () => {
    expect(isRateStale(asOf, at(121), false)).toBe(true);
  });

  it('is stale whenever the last quote request failed, even with a fresh rate', () => {
    expect(isRateStale(asOf, at(1), true)).toBe(true);
  });
});
