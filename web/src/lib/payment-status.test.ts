import { describe, expect, it } from 'vitest';
import { activeStepIndex, pollIntervalMs } from './payment-status';

describe('activeStepIndex', () => {
  it('maps the happy-path statuses onto tracker steps', () => {
    expect(activeStepIndex('PAYMENT_DETECTED')).toBe(0);
    expect(activeStepIndex('SETTLED')).toBe(1);
    expect(activeStepIndex('PAYOUT_PENDING')).toBe(2);
    expect(activeStepIndex('COMPLETED')).toBe(3);
  });

  it('returns null for statuses that are not on the tracker path', () => {
    for (const status of ['CREATED', 'QUOTED', 'AWAITING_PAYMENT', 'EXPIRED', 'FAILED', 'REFUND_PENDING', 'REFUNDED'] as const) {
      expect(activeStepIndex(status)).toBeNull();
    }
  });
});

describe('pollIntervalMs', () => {
  it('polls every 3 seconds while in progress', () => {
    expect(pollIntervalMs('AWAITING_PAYMENT')).toBe(3000);
    expect(pollIntervalMs('PAYOUT_PENDING')).toBe(3000);
  });

  it('stops on expired and refunded payments', () => {
    expect(pollIntervalMs('EXPIRED')).toBe(false);
    expect(pollIntervalMs('REFUNDED')).toBe(false);
  });

  it('keeps a slow poll after completion so a reversal is seen', () => {
    expect(pollIntervalMs('COMPLETED')).toBe(30_000);
  });
});
