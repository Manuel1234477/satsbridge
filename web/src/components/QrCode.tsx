'use client';

import { QRCodeSVG } from 'qrcode.react';

interface QrCodeProps {
  value: string;
  size?: number;
  level?: 'L' | 'M' | 'Q' | 'H';
}

export function LightningQr({ value, size = 256 }: QrCodeProps) {
  return (
    <div className="flex justify-center p-6 bg-white rounded-lg">
      <QRCodeSVG
        value={`lightning:${value}`}
        size={size}
        level="H"
        includeMargin
      />
    </div>
  );
}
