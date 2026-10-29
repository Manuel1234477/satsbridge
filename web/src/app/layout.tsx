import type { Metadata, Viewport } from 'next';
import { Inter_Tight } from 'next/font/google';
import './globals.css';
import { Providers } from './providers';
import { AppShell } from '@/components/ui/AppShell';

const sans = Inter_Tight({ subsets: ['latin'], variable: '--font-sans', display: 'swap' });

export const metadata: Metadata = {
  title: 'SatsBridge · Send across Africa',
  description: 'Send money between African currencies, settled over Lightning. Sandbox demo, no real money moves.',
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  viewportFit: 'cover',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning className={sans.variable}>
      <body>
        <Providers>
          <AppShell>{children}</AppShell>
        </Providers>
      </body>
    </html>
  );
}
