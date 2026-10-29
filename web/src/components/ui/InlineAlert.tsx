import { AlertCircle, RefreshCw } from 'lucide-react';

interface InlineAlertProps {
  message: string;
  onRetry?: () => void;
  retrying?: boolean;
}

export function InlineAlert({ message, onRetry, retrying }: InlineAlertProps) {
  return (
    <div className="inline-alert" role="alert">
      <AlertCircle className="h-4 w-4" aria-hidden="true" />
      <span className="inline-alert-text">{message}</span>
      {onRetry && (
        <button type="button" className="btn btn-ghost" onClick={onRetry} disabled={retrying} style={{ minHeight: 36 }}>
          <RefreshCw className="h-4 w-4" aria-hidden="true" />
          {retrying ? 'Retrying…' : 'Try again'}
        </button>
      )}
    </div>
  );
}
