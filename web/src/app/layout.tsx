import type { Metadata, Viewport } from 'next';
import './globals.css';
import { Providers } from './providers';
import { AppShell } from '@/components/ui/AppShell';

export const metadata: Metadata = {
  title: 'SatsBridge - Cross-border payments with Bitcoin',
  description: 'Send money across African borders, settled on Bitcoin Lightning Network.',
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  viewportFit: 'cover',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>
        <Providers>
          <AppShell>{children}</AppShell>
        </Providers>
      </body>
    </html>
  );
}
