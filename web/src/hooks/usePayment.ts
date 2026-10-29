'use client';

import { useMutation, useQuery } from '@tanstack/react-query';
import { createPayment, payPayment, getPayment, refundPayment, simulatePayout } from '@/lib/api';
import type { Recipient } from '@/lib/api/schemas';

export function useCreatePayment() {
  return useMutation({
    mutationFn: (input: { quote_id: string; recipient: Recipient }) => createPayment(input),
  });
}

export function usePayPayment() {
  return useMutation({
    mutationFn: (payment_id: string) => payPayment(payment_id),
  });
}

export function usePaymentDetails(id: string | null) {
  return useQuery({
    queryKey: ['payment', id],
    queryFn: () => (id ? getPayment(id) : Promise.reject(new Error('No payment ID'))),
    enabled: !!id,
    refetchInterval: 3000, // Poll every 3s
  });
}

export function useRefundPayment() {
  return useMutation({
    mutationFn: (input: { payment_id: string; payment_request: string }) =>
      refundPayment(input.payment_id, input.payment_request),
  });
}

export function useSimulatePayout() {
  return useMutation({
    mutationFn: (input: { payment_id: string; status: string }) =>
      simulatePayout(input.payment_id, input.status),
  });
}
