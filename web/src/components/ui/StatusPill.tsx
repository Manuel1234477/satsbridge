import { AlertCircle, Check, Clock, RotateCcw } from 'lucide-react';

type Tone = 'success' | 'pending' | 'failed' | 'refunded';

const ICON = { success: Check, pending: Clock, failed: AlertCircle, refunded: RotateCcw } as const;

// Colour is never the only signal: every pill has an icon and a text label.
export function StatusPill({ tone, label }: { tone: Tone; label: string }) {
  const Icon = ICON[tone];
  return (
    <span className={`status status-${tone}`}>
      <Icon className="h-3.5 w-3.5" aria-hidden="true" />
      {label}
    </span>
  );
}
