'use client';

import { useEffect, useState } from 'react';

export function useCountdown(expiresAt: string | null) {
  const [secondsLeft, setSecondsLeft] = useState(0);
  const [isExpired, setIsExpired] = useState(false);

  useEffect(() => {
    if (!expiresAt) {
      setSecondsLeft(0);
      setIsExpired(false);
      return;
    }

    const update = () => {
      const now = new Date();
      const expires = new Date(expiresAt);
      const delta = Math.max(0, Math.floor((expires.getTime() - now.getTime()) / 1000));

      setSecondsLeft(delta);
      setIsExpired(delta === 0);
    };

    update();
    const interval = setInterval(update, 1000);
    return () => clearInterval(interval);
  }, [expiresAt]);

  return { secondsLeft, isExpired };
}
