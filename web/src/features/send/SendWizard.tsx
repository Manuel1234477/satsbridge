'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { SendPage } from './SendPage';
import { ReviewPage } from './ReviewPage';
import type { Currency, Rail, Recipient } from '@/lib/api/schemas';

export type WizardStep = 'send' | 'recipient' | 'review' | 'payment';

interface WizardState {
  step: WizardStep;
  sourceCurrency: Currency | '';
  destCurrency: Currency | '';
  sourceAmount: number | null;
  destAmount: number | null;
  quoteId: string | null;
  paymentId: string | null;
  recipient: Recipient | null;
  recipientName: string | null;
  selectedRail: Rail | '';
}

export function SendWizard() {
  const router = useRouter();
  const [state, setState] = useState<WizardState>({
    step: 'send',
    sourceCurrency: '',
    destCurrency: '',
    sourceAmount: null,
    destAmount: null,
    quoteId: null,
    paymentId: null,
    recipient: null,
    recipientName: null,
    selectedRail: '',
  });

  const handleQuoteSelected = (
    quoteId: string,
    sourceCurrency: Currency,
    destCurrency: Currency,
    sourceAmount: number,
    destAmount: number,
    selectedRail: Rail | ''
  ) => {
    setState(prev => ({
      ...prev,
      step: 'recipient',
      quoteId,
      sourceCurrency,
      destCurrency,
      sourceAmount,
      destAmount,
      selectedRail,
    }));
  };

  const handleRecipientSelected = (recipient: Recipient, recipientName: string | null) => {
    setState(prev => ({
      ...prev,
      step: 'review',
      recipient,
      recipientName,
    }));
  };

  const handlePaymentCreated = (paymentId: string) => {
    setState(prev => ({
      ...prev,
      step: 'payment',
      paymentId,
    }));
    router.push(`/payments/${paymentId}`);
  };

  const handleBack = () => {
    if (state.step === 'recipient') {
      setState(prev => ({ ...prev, step: 'send' }));
    } else if (state.step === 'review') {
      setState(prev => ({ ...prev, step: 'recipient' }));
    }
  };

  return (
    <main className="min-h-screen bg-white dark:bg-neutral-950 py-8">
      {state.step === 'send' && (
        <SendPage onContinue={handleQuoteSelected} />
      )}
      {state.step === 'recipient' && state.quoteId && (
        <ReviewPage
          quoteId={state.quoteId}
          sourceCurrency={state.sourceCurrency as Currency}
          destCurrency={state.destCurrency as Currency}
          onPaymentCreated={handlePaymentCreated}
          onBack={handleBack}
        />
      )}
    </main>
  );
}
