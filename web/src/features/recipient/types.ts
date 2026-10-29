import type { Recipient } from '@/lib/api/schemas';

export interface RecipientDraft {
  recipient: Recipient;
  // Shown back to the sender for review. Account numbers are masked here.
  displayName: string | null;
  summary: string;
}

export function maskTail(value: string): string {
  return value.length <= 4 ? value : `${'•'.repeat(value.length - 4)}${value.slice(-4)}`;
}
