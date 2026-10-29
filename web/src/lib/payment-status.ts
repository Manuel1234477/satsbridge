import { PaymentStatus } from './api/schemas';

export interface PaymentStep {
  key: 'payment' | 'converting' | 'paying' | 'delivered';
  label: string;
  description: string;
}

const STEPS: PaymentStep[] = [
  { key: 'payment', label: 'Payment received', description: 'We received your Lightning payment' },
  { key: 'converting', label: 'Converting', description: 'Converting to local currency' },
  { key: 'paying', label: 'Paying recipient', description: 'Sending money to the recipient' },
  { key: 'delivered', label: 'Delivered', description: 'Payment complete' },
];

export function mapStatusToStep(status: PaymentStatus): PaymentStep | null {
  switch (status) {
    case 'AWAITING_PAYMENT':
    case 'PAYMENT_DETECTED':
    case 'SETTLED':
      return STEPS[0];
    case 'PAYOUT_PENDING':
      return STEPS[1];
    case 'COMPLETED':
      return STEPS[3];
    default:
      return null;
  }
}

export function getCompletedSteps(status: PaymentStatus): (PaymentStep['key'])[] {
  switch (status) {
    case 'COMPLETED':
      return ['payment', 'converting', 'paying', 'delivered'];
    case 'PAYOUT_PENDING':
      return ['payment', 'converting', 'paying'];
    case 'SETTLED':
      return ['payment', 'converting'];
    case 'PAYMENT_DETECTED':
      return ['payment'];
    default:
      return [];
  }
}

export const TERMINAL_STATUSES: PaymentStatus[] = [
  'COMPLETED',
  'EXPIRED',
  'FAILED',
  'REFUNDED',
];

export function isTerminalStatus(status: PaymentStatus): boolean {
  return TERMINAL_STATUSES.includes(status);
}
