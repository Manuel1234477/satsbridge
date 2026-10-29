import { useEffect, useState } from 'react';
import { api } from './api';
import type { CurrencyInfo } from './types';

let currenciesCache: Promise<CurrencyInfo[]> | null = null;

/** Currencies and payout rails, fetched once per page load. */
export function useCurrencies() {
  const [currencies, setCurrencies] = useState<CurrencyInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    currenciesCache ??= api.currencies();
    currenciesCache.then(setCurrencies).catch((e: Error) => {
      currenciesCache = null;
      setError(e.message);
    });
  }, []);
  return { currencies, error, loading: currencies.length === 0 && !error };
}

/** Seconds left until an ISO timestamp, ticking every second. */
export function useCountdown(iso: string | null | undefined): number {
  const target = iso ? new Date(iso).getTime() : 0;
  const [left, setLeft] = useState(() => Math.max(0, Math.ceil((target - Date.now()) / 1000)));
  useEffect(() => {
    const tick = () => setLeft(Math.max(0, Math.ceil((target - Date.now()) / 1000)));
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [target]);
  return left;
}

export function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  return `${m}:${String(seconds % 60).padStart(2, '0')}`;
}
