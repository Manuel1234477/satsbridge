'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { getPayment } from '@/lib/api';
import type { CurrencyInfo, Quote, Rail } from '@/lib/api/schemas';
import { useCurrencies } from '@/hooks/useCurrencies';
import { SendPage, type SendPrefill } from './SendPage';
import { ReviewPage } from './ReviewPage';

interface SendWizardProps {
  fromPaymentId?: string;
}

export function SendWizard({ fromPaymentId }: SendWizardProps) {
  const router = useRouter();
  const currencies = useCurrencies();
  const previous = useQuery({
    queryKey: ['payment', fromPaymentId],
    queryFn: () => getPayment(fromPaymentId ?? ''),
    enabled: fromPaymentId !== undefined,
    retry: false,
  });
  const [selected, setSelected] = useState<{ quote: Quote; rail: Rail } | null>(null);

  if (currencies.isError) {
    return <p role="alert" className="text-error-700 dark:text-error-300">{currencies.error.message}</p>;
  }
  if (!currencies.data || (fromPaymentId && previous.isPending)) {
    return <p className="text-muted">Loading…</p>;
  }

  const list: CurrencyInfo[] = currencies.data;
  const prefill: SendPrefill | undefined =
    previous.data && previous.data.status === 'EXPIRED'
      ? {
          sourceCurrency: previous.data.source_currency,
          destinationCurrency: previous.data.destination_currency,
          sourceMinor: previous.data.source_amount,
        }
      : undefined;

  if (selected) {
    return (
      <ReviewPage
        quote={selected.quote}
        rail={selected.rail}
        currencies={list}
        onBack={() => setSelected(null)}
        onPaid={(id) => router.push(`/payments/${id}`)}
      />
    );
  }

  return (
    <SendPage
      currencies={list}
      prefill={prefill}
      onContinue={(quote, rail) => setSelected({ quote, rail })}
    />
  );
}
