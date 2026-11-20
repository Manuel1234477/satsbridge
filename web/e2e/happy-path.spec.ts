import { expect, test } from '@playwright/test';

test('NGN to KES: quote, M-Pesa recipient, bank transfer, Stellar settlement, receipt (mock mode)', async ({ page }) => {
  await page.goto('/');

  await expect(page.getByRole('heading', { name: 'Send money' })).toBeVisible();
  await page.getByLabel('Amount you send').fill('150000');

  const destination = page.getByLabel('Amount they receive');
  await expect(destination).not.toHaveValue('', { timeout: 15_000 });
  await expect(page.getByText(/expires in \d+s/)).toBeVisible();
  await expect(page.getByText(/NGN → USD → KES/).first()).toBeVisible();

  await page.getByRole('button', { name: 'Continue' }).click();
  await expect(page.getByRole('heading', { name: 'Who is receiving this?' })).toBeVisible();
  await page.getByLabel('M-Pesa phone number').fill('0712345678');
  await page.getByRole('button', { name: 'Use this number' }).click();

  await expect(page.getByRole('heading', { name: 'Review and pay' })).toBeVisible();
  await page.getByRole('button', { name: 'Confirm and continue' }).click();

  await expect(page).toHaveURL(/\/payments\/pay_/);
  await expect(page.getByText('Send a bank transfer to')).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText('Sandbox Microfinance Bank')).toBeVisible();
  await expect(page.getByText('Waiting for your transfer')).toBeVisible();

  // The mock "receives" the transfer after a few seconds.
  await expect(page.getByRole('status').filter({ hasText: 'Money delivered' })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole('heading', { name: 'Receipt' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'On Stellar' })).toBeVisible();
  await expect(page.getByText('Minted and converted')).toBeVisible();
  await expect(page.getByText('Tokens burned after payout')).toBeVisible();
  await expect(page.getByText('******5678')).toBeVisible();
});
