import { expect, test } from '@playwright/test';

test('XOF to NGN bank transfer: quote, verify account, pay, and receive a receipt (mock mode)', async ({ page }) => {
  await page.goto('/');

  await expect(page.getByRole('heading', { name: 'Send money' })).toBeVisible();
  await page.getByLabel('Amount you send').fill('10000');

  const destination = page.getByLabel('Amount they receive');
  await expect(destination).not.toHaveValue('', { timeout: 15_000 });
  await expect(page.getByText(/Quote expires in \d+s/)).toBeVisible();

  await page.getByRole('button', { name: 'Continue' }).click();
  await expect(page.getByRole('heading', { name: 'Who is receiving this?' })).toBeVisible();

  const bankList = page.getByLabel('Bank list');
  await expect(bankList.locator('option', { hasText: 'GTBank' })).toBeAttached({ timeout: 15_000 });
  await bankList.selectOption({ label: 'GTBank' });
  await page.getByLabel('Account number (10 digits)').fill('0123456789');
  await page.getByRole('button', { name: 'Check account' }).click();

  await expect(page.getByRole('heading', { name: 'Confirm account holder' })).toBeVisible();
  await page.getByRole('button', { name: 'Yes, this is the account holder' }).click();

  await expect(page.getByRole('heading', { name: 'Review and pay' })).toBeVisible();
  await page.getByRole('button', { name: 'Confirm and continue' }).click();

  await expect(page).toHaveURL(/\/payments\/pay_/);
  await expect(page.getByRole('link', { name: 'Open in wallet' })).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText(/sats$/).first()).toBeVisible();

  await expect(page.getByRole('status').filter({ hasText: 'Money delivered' })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole('heading', { name: 'Receipt' })).toBeVisible();
  await expect(page.getByText('******6789')).toBeVisible();
});
