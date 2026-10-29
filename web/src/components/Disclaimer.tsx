'use client';

import { useState } from 'react';
import { X } from 'lucide-react';

export function Disclaimer() {
  const [open, setOpen] = useState(true);
  if (!open) return null;
  return (
    <aside
      aria-label="Demo notice"
      className="fixed inset-x-0 bottom-0 z-40 border-t border-warning-200 bg-warning-50 px-4 py-2 text-xs text-warning-900 dark:border-warning-800 dark:bg-warning-950 dark:text-warning-100"
    >
      <div className="mx-auto flex max-w-2xl items-start gap-3">
        <p className="flex-1">
          <strong>Demo mode.</strong> No real money is moved. Technical demonstration, not a licensed financial service.
        </p>
        <button type="button" onClick={() => setOpen(false)} className="shrink-0 rounded p-1 hover:bg-warning-100 dark:hover:bg-warning-900" aria-label="Dismiss demo notice">
          <X className="h-4 w-4" aria-hidden="true" />
        </button>
      </div>
    </aside>
  );
}
