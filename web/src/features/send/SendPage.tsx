'use client';

import { useState, useCallback, useEffect } from 'react';
import { useUser } from '@/hooks/useUser';
import { useCurrencies } from '@/hooks/useCurrencies';
import { useCreateQuote } from '@/hooks/useQuote';
import { CurrencySelector } from '@/components/CurrencySelector';
import { MoneyInput } from '@/components/MoneyInput';
import { QuoteCard } from '@/components/QuoteCard';
import { parseMoneyInput, getExponent, formatMoney } from '@/lib/money';
import { Currency, Rail } from '@/lib/api/schemas';
import { ArrowDownUp, AlertCircle } from 'lucide-react';

export function SendPage() {
  const { data: userReady } = useUser();
  const { data: currencies = [], isLoading: currenciesLoading } = useCurrencies();
  const createQuoteMutation = useCreateQuote();

  // Form state
  const [sourceCurrency, setSourceCurrency] = useState<Currency | ''>('');
  const [destCurrency, setDestCurrency] = useState<Currency | ''>('');
  const [sourceAmount, setSourceAmount] = useState<number | null>(null);
  const [destAmount, setDestAmount] = useState<number | null>(null);
  const [amountSide, setAmountSide] = useState<'SOURCE' | 'DESTINATION'>('SOURCE');
  const [sourceAmountInput, setSourceAmountInput] = useState('');
  const [destAmountInput, setDestAmountInput] = useState('');
  const [selectedRail, setSelectedRail] = useState<Rail | ''>('');
  const [error, setError] = useState<string>('');

  // Quote state
  const [quoteId, setQuoteId] = useState<string | null>(null);

  // Debounce quote creation
  useEffect(() => {
    if (!sourceCurrency || !destCurrency || sourceAmount === null && destAmount === null) {
      setQuoteId(null);
      return;
    }

    const amount = amountSide === 'SOURCE' ? sourceAmount : destAmount;
    if (amount === null || amount === 0) {
      setQuoteId(null);
      return;
    }

    const timer = setTimeout(() => {
      createQuoteMutation.mutate(
        {
          source_currency: sourceCurrency,
          destination_currency: destCurrency,
          amount,
          amount_side: amountSide,
          rail: selectedRail || undefined,
        },
        {
          onSuccess: (quote) => {
            setQuoteId(quote.id);
            setError('');
            // Update the non-editing side
            if (amountSide === 'SOURCE') {
              setDestAmount(quote.destination_amount);
              setDestAmountInput(formatMoney(quote.destination_amount, destCurrency));
            } else {
              setSourceAmount(quote.source_amount);
              setSourceAmountInput(formatMoney(quote.source_amount, sourceCurrency));
            }
          },
          onError: (err: any) => {
            setError(err.message || 'Failed to get quote');
            setQuoteId(null);
          },
        }
      );
    }, 400);

    return () => clearTimeout(timer);
  }, [sourceCurrency, destCurrency, sourceAmount, destAmount, amountSide, selectedRail, createQuoteMutation]);

  // Swap currencies
  const handleSwap = useCallback(() => {
    setSourceCurrency(destCurrency);
    setDestCurrency(sourceCurrency);
    setSourceAmount(destAmount);
    setDestAmount(sourceAmount);
    setSourceAmountInput(destAmountInput);
    setDestAmountInput(sourceAmountInput);
  }, [sourceCurrency, destCurrency, sourceAmount, destAmount, sourceAmountInput, destAmountInput]);

  // Get available rails for destination currency
  const destCurrencyInfo = currencies.find(c => c.code === destCurrency);
  const availableRails = destCurrencyInfo?.payout_rails || [];

  // Handle source amount change
  const handleSourceAmountChange = (value: number | null) => {
    setAmountSide('SOURCE');
    setSourceAmount(value);
    if (value === null) {
      setSourceAmountInput('');
      setDestAmount(null);
      setDestAmountInput('');
    } else {
      const exponent = sourceCurrency ? getExponent(sourceCurrency) : 0;
      setSourceAmountInput(formatMoney(value, sourceCurrency as Currency));
    }
  };

  // Handle destination amount change
  const handleDestAmountChange = (value: number | null) => {
    setAmountSide('DESTINATION');
    setDestAmount(value);
    if (value === null) {
      setDestAmountInput('');
      setSourceAmount(null);
      setSourceAmountInput('');
    } else {
      const exponent = destCurrency ? getExponent(destCurrency) : 0;
      setDestAmountInput(formatMoney(value, destCurrency as Currency));
    }
  };

  const isReady = userReady && !currenciesLoading && sourceCurrency && destCurrency;
  const quote = createQuoteMutation.data;

  return (
    <div className="container-safe max-w-md">
      <h1 className="text-3xl font-bold mb-8 text-center">Send money</h1>

      <form className="space-y-6">
        {/* Source section */}
        <div className="card">
          <div className="card-content space-y-2">
            <label className="block text-sm font-medium">You send</label>
            <CurrencySelector
              currencies={currencies}
              value={sourceCurrency}
              onChange={(code) => {
                setSourceCurrency(code as Currency);
                setQuoteId(null);
              }}
              disabled={!isReady}
              sendable
            />
            <MoneyInput
              value={sourceAmountInput}
              onChange={handleSourceAmountChange}
              currency={sourceCurrency as Currency}
              placeholder="0"
              disabled={!sourceCurrency}
            />
          </div>
        </div>

        {/* Swap button */}
        <div className="flex justify-center">
          <button
            type="button"
            onClick={handleSwap}
            disabled={!sourceCurrency || !destCurrency}
            className="btn btn-secondary p-2"
            aria-label="Swap currencies"
          >
            <ArrowDownUp className="w-5 h-5" />
          </button>
        </div>

        {/* Destination section */}
        <div className="card">
          <div className="card-content space-y-2">
            <label className="block text-sm font-medium">They receive</label>
            <CurrencySelector
              currencies={currencies}
              value={destCurrency}
              onChange={(code) => {
                setDestCurrency(code as Currency);
                setSelectedRail('');
                setQuoteId(null);
              }}
              disabled={!isReady}
            />

            {/* Rail selector for multi-rail currencies */}
            {availableRails.length > 1 && (
              <select
                value={selectedRail}
                onChange={(e) => {
                  setSelectedRail(e.target.value as Rail | '');
                  setQuoteId(null);
                }}
                className="form-input text-sm"
              >
                <option value="">Default payment method</option>
                {availableRails.map((rail) => (
                  <option key={rail.rail} value={rail.rail}>
                    {rail.rail.replace(/_/g, ' ')}
                  </option>
                ))}
              </select>
            )}

            <MoneyInput
              value={destAmountInput}
              onChange={handleDestAmountChange}
              currency={destCurrency as Currency}
              placeholder="0"
              disabled={!destCurrency}
            />
          </div>
        </div>

        {/* Error message */}
        {error && (
          <div className="flex gap-2 p-3 rounded-lg bg-error-50 dark:bg-error-950 text-error-700 dark:text-error-300">
            <AlertCircle className="w-5 h-5 flex-shrink-0 mt-0.5" />
            <p className="text-sm">{error}</p>
          </div>
        )}

        {/* Quote card */}
        {quote && (
          <QuoteCard
            quote={quote}
            currencies={currencies}
            isLoading={createQuoteMutation.isPending}
            onRefresh={() => setQuoteId(null)}
          />
        )}

        {/* Continue button */}
        <button
          type="submit"
          disabled={!quote || createQuoteMutation.isPending}
          className="btn btn-primary w-full"
        >
          {createQuoteMutation.isPending ? 'Getting quote...' : 'Continue to payment'}
        </button>
      </form>
    </div>
  );
}
