'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useEffect, useState, type ReactNode } from 'react';
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

function SettingsBody() {
  const [theme, setTheme] = useState<ThemeChoice>(readTheme);
  const options: { value: ThemeChoice; label: string }[] = [
    { value: 'dark', label: 'Dark' },
    { value: 'light', label: 'Light' },
    { value: 'system', label: 'Match device' },
  ];
  return (
    <div className="flex flex-col gap-5">
      <fieldset className="flex flex-col gap-2">
        <legend className="label mb-2">Appearance</legend>
        {options.map((o) => (
          <label key={o.value} className="sheet-option cursor-pointer">
            <input
              type="radio"
              name="theme"
              value={o.value}
              checked={theme === o.value}
              onChange={() => {
                setTheme(o.value);
                setThemeChoice(o.value);
              }}
            />
            {o.label}
          </label>
        ))}
      </fieldset>
      <div className="card-raised text-sm">
        <p className="font-semibold">Language</p>
        <p className="mt-1 text-[var(--text-2)]">English only for now. French is not built yet.</p>
      </div>
      <div className="card-raised text-sm">
        <p className="font-semibold">Demo mode</p>
        <p className="mt-1 text-[var(--text-2)]">Technical demonstration. Not a licensed financial service. No real money is moved.</p>
      </div>
    </div>
  );
}

export function AppShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [demoOpen, setDemoOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const paymentFlow = pathname.startsWith('/payments/');

  useEffect(() => {
    applyTheme(readTheme());
  }, []);

  return (
    <div className="app-frame">
      <div className="app-column">
        <header className="topbar">
          {paymentFlow ? (
            <Link href="/" className="btn btn-ghost" aria-label="Back to send">
              ← Back
            </Link>
          ) : (
            <span className="flex items-center gap-2 font-semibold">
              <span className="badge" aria-hidden="true">
                SB
              </span>
              SatsBridge
            </span>
          )}
          <button type="button" className="demo-pill" aria-haspopup="dialog" onClick={() => setDemoOpen(true)}>
            Demo
          </button>
        </header>

        <main id="main">{children}</main>

        {!paymentFlow && (
          <nav className="tabbar" aria-label="Main">
            <Link href="/" className="tab" aria-current={pathname === '/' ? 'page' : undefined}>
              Send
            </Link>
            <Link href="/history" className="tab" aria-current={pathname.startsWith('/history') ? 'page' : undefined}>
              Activity
            </Link>
            <button type="button" className="tab" aria-haspopup="dialog" onClick={() => setSettingsOpen(true)}>
              Settings
            </button>
          </nav>
        )}

        <Sheet open={demoOpen} onClose={() => setDemoOpen(false)} title="Demo mode">
          <p>Technical demonstration. Not a licensed financial service. No real money is moved.</p>
        </Sheet>
        <Sheet open={settingsOpen} onClose={() => setSettingsOpen(false)} title="Settings">
          <SettingsBody />
        </Sheet>
      </div>
    </div>
  );
}
