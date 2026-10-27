'use client';

import { useQuery } from '@tanstack/react-query';
import { getPayment } from '@/lib/api';
import { pollIntervalMs } from '@/lib/payment-status';

export function usePaymentDetails(id: string) {
  return useQuery({
    queryKey: ['payment', id],
    queryFn: () => getPayment(id),
    refetchInterval: (query) => pollIntervalMs(query.state.data?.status),
    retry: false,
  });
}
