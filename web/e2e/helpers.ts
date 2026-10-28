import { expect, type Page } from '@playwright/test';

// Walks the sender flow to the payment page for a given bank account (mock magic values).
export async function payToAccount(page: Page, account: string): Promise<void> {
  await page.goto('/');
  await page.getByLabel('Amount you send').fill('10000');
  await expect(page.getByLabel('Amount they receive')).not.toHaveValue('', { timeout: 15_000 });
  await page.getByRole('button', { name: 'Continue' }).click();

  await page.getByRole('button', { name: 'Choose bank' }).click();
  await page.getByRole('option', { name: 'GTBank' }).click();
  await page.getByLabel('Account number (10 digits)').fill(account);
  await page.getByRole('button', { name: 'Check account' }).click();
  await page.getByRole('button', { name: 'Yes, this is correct' }).click();
  await page.getByRole('button', { name: 'Confirm and continue' }).click();
  await expect(page).toHaveURL(/\/payments\/pay_/);
}
