export const DEFAULT_ADDRESS_DOMAIN = 'tapnob.io';

const ADDRESS = /^[a-z0-9._+-]+@[a-z0-9-]+(\.[a-z0-9-]+)+$/;
const USERNAME = /^[a-z0-9._+-]+$/;

// Turns what the sender typed into the address to pay, or null if it can't be one.
// "Ada", "ada@tapnob.io" and "lightning:ada@tapnob.io" all become "ada@tapnob.io".
export function normalizeLightningAddress(input: string): string | null {
  const value = input.trim().toLowerCase().replace(/^lightning:/, '');
  if (value === '') return null;
  if (USERNAME.test(value)) return `${value}@${DEFAULT_ADDRESS_DOMAIN}`;
  return ADDRESS.test(value) ? value : null;
}
