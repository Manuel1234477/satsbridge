import { defineConfig } from '@playwright/test';

const chrome = process.env.PLAYWRIGHT_CHROME_PATH ?? '/usr/bin/google-chrome';

export default defineConfig({
  testDir: 'e2e',
  globalSetup: './e2e/global-setup.ts',
  timeout: 120_000,
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: 'http://localhost:3100',
    launchOptions: { executablePath: chrome },
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'npm run dev -- -p 3100',
    url: 'http://localhost:3100',
    reuseExistingServer: true,
    timeout: 120_000,
    env: {
      NEXT_PUBLIC_MOCK_API: 'true',
      NEXT_PUBLIC_MOCK_TIME_SCALE: '0.2',
    },
  },
});
