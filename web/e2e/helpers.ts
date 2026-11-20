import { expect, type Page } from '@playwright/test';

// Walks the sender flow (NGN -> KES, M-Pesa) to the bank transfer page. The number's last four
// digits pick the mock outcome (see src/lib/api/mock.ts).
export async function payToPhone(page: Page, phone: string): Promise<void> {
  await page.goto('/');
  await page.getByLabel('Amount you send').fill('150000');
  await expect(page.getByLabel('Amount they receive')).not.toHaveValue('', { timeout: 15_000 });
  await page.getByRole('button', { name: 'Continue' }).click();

  await page.getByLabel('M-Pesa phone number').fill(phone);
  await page.getByRole('button', { name: 'Use this number' }).click();
  await page.getByRole('button', { name: 'Confirm and continue' }).click();
  await expect(page).toHaveURL(/\/payments\/pay_/);
  await expect(page.getByText('Send a bank transfer to')).toBeVisible({ timeout: 15_000 });
}
