'use client';

import { useMutation, useQuery } from '@tanstack/react-query';
import { createQuote, getQuote } from '@/lib/api';
import type { Currency, Rail } from '@/lib/api/schemas';

interface CreateQuoteInput {
  source_currency: Currency;
  destination_currency: Currency;
  amount: number;
  amount_side: 'SOURCE' | 'DESTINATION';
  rail?: Rail;
}

export function useCreateQuote() {
  return useMutation({
    mutationFn: (input: CreateQuoteInput) => createQuote(input),
  });
}

export function useQuote(id: string | null) {
  return useQuery({
    queryKey: ['quote', id],
    queryFn: () => (id ? getQuote(id) : Promise.reject(new Error('No quote ID'))),
    enabled: !!id,
    staleTime: 0, // Always fresh
  });
}
