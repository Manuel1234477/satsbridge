import type { PayoutInstitution } from './api';
import type { Rail } from './api/schemas';

function countryName(code: string): string {
  try {
    return new Intl.DisplayNames(['en'], { type: 'region' }).of(code.toUpperCase()) ?? code;
  } catch {
    return code;
  }
}

// The backend masks the destination as `<bank_code>:<masked account>` (bank transfer) or
// `<country>:<provider>:<masked phone>` (mobile money). Swap the codes for names when they are known.
export function formatRecipientAccount(masked: string, rail: Rail, institutions: PayoutInstitution[] | undefined): string {
  const parts = masked.split(':');
  const nameOf = (code: string) => institutions?.find((i) => i.code === code)?.name;
  if (rail === 'NG_BANK_TRANSFER' && parts.length === 2) {
    const bank = nameOf(parts[0]);
    return bank ? `${bank} · ${parts[1]}` : masked;
  }
  if (rail === 'MOBILE_MONEY' && parts.length === 3) {
    const provider = nameOf(parts[1]) ?? parts[1];
    return `${provider} · ${countryName(parts[0])} · ${parts[2]}`;
  }
  return masked;
}
