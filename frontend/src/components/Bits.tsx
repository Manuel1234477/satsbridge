import QRCode from 'qrcode';
import { useEffect, useState } from 'react';
import { STATUS_LABEL, STATUS_TONE } from '../lib/labels';
import type { PaymentStatus } from '../lib/types';

export function StatusBadge({ status }: { status: PaymentStatus }) {
  return <span className={`badge badge-${STATUS_TONE[status]}`}>{STATUS_LABEL[status]}</span>;
}

export function Qr({ value, label }: { value: string; label: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    QRCode.toDataURL(value, { margin: 1, width: 280, errorCorrectionLevel: 'L' })
      .then((url) => alive && setSrc(url))
      .catch(() => alive && setSrc(null));
    return () => {
      alive = false;
    };
  }, [value]);
  return src ? <img className="qr" src={src} alt={label} width={280} height={280} /> : <div className="qr qr-empty" />;
}

export function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      type="button"
      className="btn btn-ghost"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setDone(true);
          setTimeout(() => setDone(false), 1500);
        } catch {
          // Clipboard blocked: the text is still selectable on screen.
        }
      }}
    >
      {done ? 'Copied' : label}
    </button>
  );
}

export function ErrorNote({ children }: { children: React.ReactNode }) {
  return (
    <p className="note note-bad" role="alert">
      {children}
    </p>
  );
}
