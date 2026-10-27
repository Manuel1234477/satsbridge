'use client';

import { useMutation } from '@tanstack/react-query';
import { resolveRecipient } from '@/lib/api';

export function useRecipientResolution() {
  return useMutation({
    mutationFn: (input: { currency: string; bank_code: string; account_number: string }) =>
      resolveRecipient(input),
  });
}
