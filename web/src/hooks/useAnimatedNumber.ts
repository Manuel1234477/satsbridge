'use client';

import { useEffect, useRef, useState } from 'react';
import { useReducedMotion } from './useMediaQuery';

const DURATION_MS = 220;

// Display-only count-up between two server amounts (integer minor units). Never used for money math.
export function useAnimatedNumber(target: number | undefined): number | undefined {
  const reduced = useReducedMotion();
  const [shown, setShown] = useState(target);
  const from = useRef(target);

  useEffect(() => {
    if (target === undefined) return;
    const start = from.current;
    if (reduced || start === undefined || start === target) {
      from.current = target;
      const id = requestAnimationFrame(() => setShown(target));
      return () => cancelAnimationFrame(id);
    }
    const began = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - began) / DURATION_MS);
      const eased = 1 - (1 - t) ** 3;
      setShown(Math.round(start + (target - start) * eased));
      if (t < 1) frame = requestAnimationFrame(tick);
      else from.current = target;
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      from.current = target;
    };
  }, [target, reduced]);

  return target === undefined ? undefined : shown;
}
