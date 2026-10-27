'use client';

import { useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ensureUser } from '@/lib/api';

export function useUser() {
  useEffect(() => {
    ensureUser().catch(console.error);
  }, []);

  return useQuery({
    queryKey: ['user'],
    queryFn: async () => {
      return { ready: true };
    },
    staleTime: Infinity,
  });
}
