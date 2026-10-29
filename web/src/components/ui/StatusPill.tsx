type Tone = 'success' | 'pending' | 'failed' | 'refunded';

const ICON: Record<Tone, string> = { success: '✓', pending: '…', failed: '!', refunded: '↺' };

// Colour is never the only signal: every pill has an icon and a text label.
export function StatusPill({ tone, label }: { tone: Tone; label: string }) {
  return (
    <span className={`status status-${tone}`}>
      <span aria-hidden="true">{ICON[tone]}</span>
      {label}
    </span>
  );
}
