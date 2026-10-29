import type { PaymentDetails, PaymentStatus } from './api/schemas';

export const TRACKER_STEPS = ['received', 'converting', 'paying', 'delivered'] as const;
export type TrackerStep = (typeof TRACKER_STEPS)[number];

// Index of the step in progress for a payment on the happy path.
// Null for statuses that are not on the path (not yet paid, expired, failed, refunds).
export function activeStepIndex(status: PaymentStatus): number | null {
  switch (status) {
    case 'PAYMENT_DETECTED':
      return 0;
    case 'SETTLED':
      return 1;
    case 'PAYOUT_PENDING':
      return 2;
    case 'COMPLETED':
      return 3;
    default:
      return null;
  }
}

export function isReversal(payment: PaymentDetails): boolean {
  return payment.status === 'FAILED' && payment.payout?.status === 'reversed';
}

// Polling stops on these. COMPLETED keeps a slow poll elsewhere so a later reversal is seen.
export const STOP_POLLING_STATUSES: readonly PaymentStatus[] = ['EXPIRED', 'REFUNDED'];

export function pollIntervalMs(status: PaymentStatus | undefined): number | false {
  if (!status) return 3000;
  if (STOP_POLLING_STATUSES.includes(status)) return false;
  if (status === 'COMPLETED') return 30_000;
  return 3000;
}
