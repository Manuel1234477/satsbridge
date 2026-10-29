import { expect, test } from '@playwright/test';
import { mkdirSync } from 'fs';

// Opt-in: SHOTS=1 npx playwright test e2e/screenshots.spec.ts
const OUT = process.env.SHOTS_DIR ?? '/tmp/satsbridge-shots';

const VIEWPORTS = [
  { name: 'mobile-390', width: 390, height: 844 },
  { name: 'desktop-1280', width: 1280, height: 900 },
];

for (const vp of VIEWPORTS) {
  test(`capture states at ${vp.name}`, async ({ page }) => {
    test.skip(!process.env.SHOTS, 'set SHOTS=1 to capture screenshots');
    mkdirSync(OUT, { recursive: true });
    await page.setViewportSize({ width: vp.width, height: vp.height });

    await page.goto('/');
    await page.getByLabel('Amount you send').fill('10000');
    await expect(page.getByLabel('Amount they receive')).not.toHaveValue('', { timeout: 15_000 });
    await expect(page.getByText(/Expires in \d+s/)).toBeVisible();
    await page.screenshot({ path: `${OUT}/${vp.name}-1-send.png`, fullPage: false });

    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('button', { name: 'Choose bank' }).click();
    await page.getByRole('option', { name: 'GTBank' }).click();
    await page.getByLabel('Account number (10 digits)').fill('0123456789');
    await page.getByRole('button', { name: 'Check account' }).click();
    await expect(page.getByRole('heading', { name: 'Confirm account holder' })).toBeVisible();
    await page.screenshot({ path: `${OUT}/${vp.name}-2-recipient.png`, fullPage: false });

    await page.getByRole('button', { name: 'Yes, this is correct' }).click();
    await page.getByRole('button', { name: 'Confirm and continue' }).click();
    await expect(page.getByRole('link', { name: 'Open in wallet' })).toBeVisible({ timeout: 15_000 });
    await page.screenshot({ path: `${OUT}/${vp.name}-3-pay.png`, fullPage: false });

    await expect(page.getByRole('status').filter({ hasText: 'Money delivered' })).toBeVisible({ timeout: 60_000 });
    await page.screenshot({ path: `${OUT}/${vp.name}-4-tracker-success.png`, fullPage: true });

    await page.goto('/history');
    await expect(page.getByRole('heading', { name: 'Activity' })).toBeVisible();
    await page.screenshot({ path: `${OUT}/${vp.name}-5-activity.png`, fullPage: false });
  });
}
