'use client';

import { useQuery } from '@tanstack/react-query';
import { createQuote, getQuote, type CreateQuoteInput } from '@/lib/api';

export function useQuotePreview(input: CreateQuoteInput | null) {
  return useQuery({
    queryKey: ['quote-preview', input],
    queryFn: () => {
      if (!input) throw new Error('No quote input');
      return createQuote(input);
    },
    enabled: input !== null,
    retry: false,
    gcTime: 0,
  });
}

export function useQuoteById(id: string | null) {
  return useQuery({
    queryKey: ['quote', id],
    queryFn: () => {
      if (!id) throw new Error('No quote id');
      return getQuote(id);
    },
    enabled: id !== null,
    retry: false,
  });
}
