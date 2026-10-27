'use client';

import { useState } from 'react';
import { AlertCircle, X } from 'lucide-react';

export function Disclaimer() {
  const [show, setShow] = useState(true);

  if (!show) return null;

  return (
    <div className="fixed bottom-0 left-0 right-0 bg-warning-50 dark:bg-warning-950 border-t border-warning-200 dark:border-warning-800 p-4 z-40 max-h-32 overflow-y-auto">
      <div className="container-safe max-w-4xl flex gap-3">
        <AlertCircle className="w-5 h-5 text-warning-700 dark:text-warning-300 flex-shrink-0 mt-0.5" />
        <div className="flex-1 text-sm text-warning-800 dark:text-warning-200">
          <p className="font-medium mb-1">Demo mode</p>
          <p>
            This is a technical demonstration. It is not a licensed financial service. No real money is moved.
            For production use, see the <a href="#" className="underline hover:no-underline">full disclaimer</a>.
          </p>
        </div>
        <button
          onClick={() => setShow(false)}
          className="flex-shrink-0 p-1 hover:bg-warning-100 dark:hover:bg-warning-900 rounded"
          aria-label="Dismiss"
        >
          <X className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}
