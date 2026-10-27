import type { Metadata } from 'next';
import './globals.css';
import { Providers } from './providers';
import { Disclaimer } from '@/components/Disclaimer';

export const metadata: Metadata = {
  title: 'SatsBridge - Cross-border payments with Bitcoin',
  description: 'Send money across African borders, settled on Bitcoin Lightning Network.',
  viewport: 'width=device-width, initial-scale=1, viewport-fit=cover',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="pb-20 sm:pb-0">
        <Providers>
          {children}
          <Disclaimer />
        </Providers>
      </body>
    </html>
  );
}
