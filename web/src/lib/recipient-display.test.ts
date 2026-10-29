import { describe, expect, it } from 'vitest';
import type { PayoutInstitution } from './api';
import { formatRecipientAccount } from './recipient-display';

const banks: PayoutInstitution[] = [{ code: '000014', name: 'Access Bank', kind: 'bank' }];
const operators: PayoutInstitution[] = [{ code: 'WAVE', name: 'Wave', kind: 'mobile_money' }];

describe('formatRecipientAccount', () => {
  it('shows the bank name instead of its code', () => {
    expect(formatRecipientAccount('000014:******6789', 'NG_BANK_TRANSFER', banks)).toBe('Access Bank · ******6789');
  });
  it('keeps the masked value when the bank is unknown or the list is not loaded', () => {
    expect(formatRecipientAccount('999:******6789', 'NG_BANK_TRANSFER', banks)).toBe('999:******6789');
    expect(formatRecipientAccount('000014:******6789', 'NG_BANK_TRANSFER', undefined)).toBe('000014:******6789');
  });
  it('shows provider and country for mobile money', () => {
    expect(formatRecipientAccount('SN:WAVE:*****4567', 'MOBILE_MONEY', operators)).toBe('Wave · Senegal · *****4567');
  });
  it('leaves other rails alone', () => {
    expect(formatRecipientAccount('ada@tapnob.io', 'LIGHTNING_ADDRESS', banks)).toBe('ada@tapnob.io');
  });
});
