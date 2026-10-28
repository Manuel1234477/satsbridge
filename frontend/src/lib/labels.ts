import type { PaymentStatus, Rail } from './types';

export const RAIL_LABEL: Record<Rail, string> = {
  NG_BANK_TRANSFER: 'Bank account',
  LIGHTNING_ADDRESS: 'Lightning Address',
  MOBILE_MONEY: 'Mobile money',
  KE_MOBILE_MONEY: 'M-Pesa',
};

export const RAIL_HINT: Record<Rail, string> = {
  NG_BANK_TRANSFER: 'Instant transfer to a Nigerian bank account',
  LIGHTNING_ADDRESS: 'Paid to name@tapnob.io, which converts to naira',
  MOBILE_MONEY: 'Paid to a mobile money wallet',
  KE_MOBILE_MONEY: 'Paid to an M-Pesa number',
};

export const STATUS_LABEL: Record<PaymentStatus, string> = {
  CREATED: 'Created',
  QUOTED: 'Waiting for payment',
  AWAITING_PAYMENT: 'Waiting for payment',
  PAYMENT_DETECTED: 'Payment detected',
  SETTLED: 'Payment received',
  PAYOUT_PENDING: 'Sending payout',
  COMPLETED: 'Delivered',
  EXPIRED: 'Expired',
  FAILED: 'Failed',
  REFUND_PENDING: 'Refund due',
  REFUNDED: 'Refunded',
};

export type Tone = 'info' | 'ok' | 'warn' | 'bad';

export const STATUS_TONE: Record<PaymentStatus, Tone> = {
  CREATED: 'info',
  QUOTED: 'info',
  AWAITING_PAYMENT: 'info',
  PAYMENT_DETECTED: 'info',
  SETTLED: 'info',
  PAYOUT_PENDING: 'info',
  COMPLETED: 'ok',
  EXPIRED: 'warn',
  FAILED: 'bad',
  REFUND_PENDING: 'warn',
  REFUNDED: 'ok',
};

/** Statuses after which nothing more will happen without user action. */
export const SETTLED_STATES: PaymentStatus[] = ['COMPLETED', 'EXPIRED', 'REFUNDED'];

export function humanizeEvent(type: string): string {
  const text = type.replace(/[._]/g, ' ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export function formatDateTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}
