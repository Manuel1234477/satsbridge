'use client';

import { QRCodeSVG } from 'qrcode.react';

export function LightningQr({ value, size = 240 }: { value: string; size?: number }) {
  return <QRCodeSVG value={`lightning:${value}`} size={size} level="H" includeMargin />;
}
