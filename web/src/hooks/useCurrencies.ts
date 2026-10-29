'use client';

import { useQuery } from '@tanstack/react-query';
import { getCurrencies } from '@/lib/api';

export function useCurrencies() {
  return useQuery({
    queryKey: ['currencies'],
    queryFn: getCurrencies,
    staleTime: 1000 * 60 * 60, // 1 hour
  });
}
