import { describe, expect, it } from 'vitest';
import { normalizeLightningAddress } from './lightning-address';

describe('normalizeLightningAddress', () => {
  it('adds the Tapnob domain to a bare username', () => {
    expect(normalizeLightningAddress('ada')).toBe('ada@tapnob.io');
    expect(normalizeLightningAddress('  Ada.Okafor ')).toBe('ada.okafor@tapnob.io');
  });

  it('keeps a full address and lowercases it', () => {
    expect(normalizeLightningAddress('Ada@Tapnob.io')).toBe('ada@tapnob.io');
  });

  it('strips a pasted lightning: prefix', () => {
    expect(normalizeLightningAddress('lightning:ada@tapnob.io')).toBe('ada@tapnob.io');
  });

  it('rejects input that cannot be an address', () => {
    expect(normalizeLightningAddress('')).toBeNull();
    expect(normalizeLightningAddress('ada@')).toBeNull();
    expect(normalizeLightningAddress('ada@tapnob')).toBeNull();
    expect(normalizeLightningAddress('a da@tapnob.io')).toBeNull();
  });
});
