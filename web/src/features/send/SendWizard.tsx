'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { getPayment } from '@/lib/api';
import type { CurrencyInfo, Quote, Rail } from '@/lib/api/schemas';
import { useCurrencies } from '@/hooks/useCurrencies';
import { InlineAlert } from '@/components/ui/InlineAlert';
import { Skeleton } from '@/components/ui/Skeleton';
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
    return (
      <div className="page">
        <InlineAlert message={currencies.error.message} onRetry={() => void currencies.refetch()} retrying={currencies.isFetching} />
      </div>
    );
  }
  if (!currencies.data || (fromPaymentId && previous.isPending)) {
    return (
      <div className="page flex flex-col gap-5" role="status" aria-label="Loading">
        <Skeleton height={220} />
        <Skeleton height={160} />
        <Skeleton height={56} />
      </div>
    );
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
