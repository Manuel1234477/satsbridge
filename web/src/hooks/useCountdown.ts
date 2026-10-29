'use client';

import { useNow } from './useNow';

// Display-only countdown to an ISO timestamp. The server decides when something expires.
export function useCountdown(expiresAt: string | null | undefined): { secondsLeft: number; isExpired: boolean } {
  const now = useNow();
  if (!expiresAt) return { secondsLeft: 0, isExpired: false };
  const secondsLeft = Math.max(0, Math.ceil((new Date(expiresAt).getTime() - now) / 1000));
  return { secondsLeft, isExpired: secondsLeft === 0 };
}
