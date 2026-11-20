import { expect, test } from '@playwright/test';
import { payToPhone } from './helpers';

test('failed payout is reversed on Stellar and refunded to the sender in naira', async ({ page }) => {
  await payToPhone(page, '0712340000');

  await expect(page.getByRole('heading', { name: 'Your refund' })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByText(/Refunding .*NGN/)).toBeVisible();
  await expect(page.getByText('Conversion reversed')).toBeVisible();
  await expect(page.getByRole('status').filter({ hasText: 'Refunded' })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole('heading', { name: 'Refund details' })).toBeVisible();
});

test('completed payment that the provider reverses shows a reversal state', async ({ page }) => {
  await payToPhone(page, '0712341111');

  await expect(page.getByRole('status').filter({ hasText: 'Money delivered' })).toBeVisible({ timeout: 60_000 });
  // COMPLETED is polled every 30 seconds, so the reversal appears within one interval.
  await expect(page.getByRole('status').filter({ hasText: /Payment reversed|Refunded/ })).toBeVisible({ timeout: 90_000 });
});
