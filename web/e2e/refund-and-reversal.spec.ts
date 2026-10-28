import { expect, test } from '@playwright/test';
import { payToAccount } from './helpers';

test('failed payout asks for an exact-amount refund invoice and ends REFUNDED', async ({ page }) => {
  await payToAccount(page, '0000000000');

  await expect(page.getByRole('heading', { name: 'Get your refund' })).toBeVisible({ timeout: 60_000 });
  const instruction = await page.getByText(/Paste a Lightning invoice for exactly/).textContent();
  const sats = Number(/exactly ([\d,]+) sats/.exec(instruction ?? '')?.[1].replace(/,/g, ''));
  expect(sats).toBeGreaterThan(0);

  await page.getByLabel('Refund invoice').fill(`lnbc${sats * 10 + 1}n1mock`);
  await page.getByRole('button', { name: 'Send refund' }).click();
  await expect(page.locator('p[role="alert"]')).toContainText(`exactly ${sats} sats`);

  await page.getByLabel('Refund invoice').fill(`lnbc${sats * 10}n1mock`);
  await page.getByRole('button', { name: 'Send refund' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Refunded' })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole('heading', { name: 'Refund details' })).toBeVisible();
});

test('completed payment that the provider reverses shows a reversal state', async ({ page }) => {
  await payToAccount(page, '1111111111');

  await expect(page.getByRole('status').filter({ hasText: 'Money delivered' })).toBeVisible({ timeout: 60_000 });
  // COMPLETED is polled every 30 seconds, so the reversal appears within one interval.
  await expect(page.getByRole('status').filter({ hasText: 'Payment reversed' })).toBeVisible({ timeout: 90_000 });
  await expect(page.getByRole('heading', { name: 'Get your refund' })).toBeVisible({ timeout: 30_000 });
});
