'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useEffect, useState, type ReactNode } from 'react';
import { ArrowLeft, FlaskConical, History, Moon, Send, Settings, Sun } from 'lucide-react';
import { LogoMark } from './LogoMark';
import { Sheet } from './Sheet';

export type ThemeChoice = 'dark' | 'light' | 'system';
const THEME_KEY = 'satsbridge.theme';

function readTheme(): ThemeChoice {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    return stored === 'light' || stored === 'system' ? stored : 'dark';
  } catch {
    return 'dark';
  }
}

function applyTheme(theme: ThemeChoice): void {
  document.documentElement.dataset.theme = theme;
}

export function setThemeChoice(theme: ThemeChoice): void {
  applyTheme(theme);
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Storage blocked: the choice lasts for this page only.
  }
}

const SANDBOX_TEXT = 'Technical demonstration. Not a licensed financial service. No real money is moved.';

function ThemeOptions({ theme, onChange }: { theme: ThemeChoice; onChange: (t: ThemeChoice) => void }) {
  const options: { value: ThemeChoice; label: string }[] = [
    { value: 'dark', label: 'Dark' },
    { value: 'light', label: 'Light' },
    { value: 'system', label: 'Device' },
  ];
  return (
    <div className="segmented" role="radiogroup" aria-label="Appearance">
      {options.map((o) => (
        <button key={o.value} type="button" role="radio" aria-checked={theme === o.value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function AppShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [sandboxOpen, setSandboxOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [theme, setTheme] = useState<ThemeChoice>('dark');
  const paymentFlow = pathname.startsWith('/payments/');

  useEffect(() => {
    const stored = readTheme();
    applyTheme(stored);
    // Sync React state with the stored choice after mount; reading storage during render would mismatch SSR.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setTheme(stored);
  }, []);

  const chooseTheme = (next: ThemeChoice) => {
    setTheme(next);
    setThemeChoice(next);
  };
  const isLight = theme === 'light';

  return (
    <div className="app-frame">
      <div className="app-column">
        <header className="topbar">
          <div className="flex items-center gap-6">
            {paymentFlow ? (
              <Link href="/" className="btn btn-ghost" aria-label="Back to send">
                <ArrowLeft className="h-5 w-5" aria-hidden="true" />
                Back
              </Link>
            ) : (
              <Link href="/" className="flex items-center gap-2.5 text-[17px] font-semibold tracking-tight text-[var(--text)] no-underline">
                <LogoMark size={30} />
                SatsBridge
              </Link>
            )}
            <nav className="topnav" aria-label="Primary">
              <Link href="/" aria-current={pathname === '/' ? 'page' : undefined}>
                Send
              </Link>
              <Link href="/history" aria-current={pathname.startsWith('/history') ? 'page' : undefined}>
                Activity
              </Link>
            </nav>
          </div>
          <div className="topbar-actions">
            <button
              type="button"
              className="btn btn-ghost"
              aria-label={isLight ? 'Switch to dark theme' : 'Switch to light theme'}
              onClick={() => chooseTheme(isLight ? 'dark' : 'light')}
            >
              {isLight ? <Moon className="h-5 w-5" aria-hidden="true" /> : <Sun className="h-5 w-5" aria-hidden="true" />}
            </button>
            <button type="button" className="sandbox-badge" aria-haspopup="dialog" onClick={() => setSandboxOpen(true)}>
              <FlaskConical className="h-3.5 w-3.5" aria-hidden="true" />
              Sandbox
            </button>
          </div>
        </header>

        <main id="main">{children}</main>

        {!paymentFlow && (
          <nav className="tabbar" aria-label="Main">
            <Link href="/" className="tab" aria-current={pathname === '/' ? 'page' : undefined}>
              <Send className="h-5 w-5" aria-hidden="true" />
              Send
            </Link>
            <Link href="/history" className="tab" aria-current={pathname.startsWith('/history') ? 'page' : undefined}>
              <History className="h-5 w-5" aria-hidden="true" />
              Activity
            </Link>
            <button type="button" className="tab" aria-haspopup="dialog" onClick={() => setSettingsOpen(true)}>
              <Settings className="h-5 w-5" aria-hidden="true" />
              Settings
            </button>
          </nav>
        )}

        <Sheet open={sandboxOpen} onClose={() => setSandboxOpen(false)} title="Sandbox">
          <p>{SANDBOX_TEXT}</p>
          <p className="label">Rates, invoices and payouts come from the backend or, in mock mode, from an in-browser stand-in.</p>
        </Sheet>
        <Sheet open={settingsOpen} onClose={() => setSettingsOpen(false)} title="Settings">
          <div className="flex flex-col gap-5">
            <div className="flex flex-col gap-2">
              <span className="label">Appearance</span>
              <ThemeOptions theme={theme} onChange={chooseTheme} />
            </div>
            <div className="card-raised text-sm">
              <p className="font-semibold">Language</p>
              <p className="label mt-1">English only. French is not built yet.</p>
            </div>
            <div className="card-raised text-sm">
              <p className="font-semibold">Sandbox</p>
              <p className="label mt-1">{SANDBOX_TEXT}</p>
            </div>
          </div>
        </Sheet>
      </div>
    </div>
  );
}
