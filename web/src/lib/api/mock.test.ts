import { beforeAll, describe, expect, it } from 'vitest';
import {
  CurrencyInfoSchema,
  DepositSchema,
  PayoutInstitutionSchema,
  PaymentDetailsSchema,
  PaymentIntentSchema,
  QuoteSchema,
  RecipientResolutionSchema,
  UserSchema,
} from './schemas';

type MockModule = typeof import('./mock');
let mock: MockModule;

async function call(method: string, path: string, body: unknown, token?: string) {
  const res = await mock.mockRespond(method, path, body, token);
  return { status: res.status, data: (await res.json()) as Record<string, unknown> };
}

async function signup(): Promise<string> {
  const { data } = await call('POST', '/users', {});
  return data.token as string;
}

// ₦ amount in kobo -> KES, to an M-Pesa number.
async function kesPayment(token: string, amount: number, phone: string) {
  const q = await call('POST', '/quotes', { source_currency: 'NGN', destination_currency: 'KES', amount, amount_side: 'source' }, token);
  expect(q.status).toBe(201);
  const payment = await call('POST', '/payments', { quote_id: q.data.id, recipient: { rail: 'KE_MOBILE_MONEY', phone_number: phone } }, token);
  expect(payment.status).toBe(201);
  return { quote: q.data, id: payment.data.id as string, payment: payment.data };
}

async function waitFor(token: string, id: string, status: string) {
  const seen = new Set<string>();
  for (let i = 0; i < 300; i++) {
    const details = await call('GET', `/payments/${id}`, undefined, token);
    expect(PaymentDetailsSchema.safeParse(details.data).success).toBe(true);
    seen.add(details.data.status as string);
    if (details.data.status === status) return { details: details.data, seen };
    await wait(10);
  }
  throw new Error(`never reached ${status}; saw ${[...seen].join(', ')}`);
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

beforeAll(async () => {
  process.env.NEXT_PUBLIC_MOCK_TIME_SCALE = '0.01';
  mock = await import('./mock');
});

describe('mock API', () => {
  it('returns payloads that match the Zod schemas the client validates with', async () => {
    const user = await call('POST', '/users', {});
    expect(UserSchema.safeParse(user.data).success).toBe(true);
    const currencies = await call('GET', '/currencies', undefined);
    expect(CurrencyInfoSchema.array().safeParse(currencies.data).success).toBe(true);
    const list = currencies.data as unknown as { code: string; can_send: boolean }[];
    expect(list.filter((c) => c.can_send).map((c) => c.code)).toEqual(['NGN']);
    const resolved = await call('POST', '/recipients/resolve', { currency: 'NGN', bank_code: '058', account_number: '0123456789' }, user.data.token as string);
    expect(RecipientResolutionSchema.safeParse(resolved.data).success).toBe(true);
  });

  it('only accepts naira as the source currency', async () => {
    const token = await signup();
    const bad = await call('POST', '/quotes', { source_currency: 'KES', destination_currency: 'NGN', amount: 1000, amount_side: 'source' }, token);
    expect(bad.status).toBe(400);
    const same = await call('POST', '/quotes', { source_currency: 'NGN', destination_currency: 'NGN', amount: 1000, amount_side: 'source' }, token);
    expect(same.status).toBe(400);
  });

  it('accepts a mobile money recipient for GHS and rejects a malformed number', async () => {
    const token = await signup();
    const { data: q } = await call('POST', '/quotes', { source_currency: 'NGN', destination_currency: 'GHS', amount: 5_000_000, amount_side: 'source' }, token);
    expect(q.conversion_path).toEqual(['NGN', 'USD', 'GHS']);
    const recipient = { rail: 'MOBILE_MONEY', country: 'GH', provider_code: 'MTN', phone_number: '0241234567' };
    const bad = await call('POST', '/payments', { quote_id: q.id, recipient: { ...recipient, phone_number: '12' } }, token);
    expect(bad.status).toBe(422);
    const ok = await call('POST', '/payments', { quote_id: q.id, recipient }, token);
    expect(ok.status).toBe(201);
    expect(PaymentIntentSchema.safeParse(ok.data).success).toBe(true);
    expect(ok.data.recipient_masked).toBe('GH:MTN:******4567');
  });

  it('requires a country for mobile money operators, like the backend', async () => {
    const missing = await call('GET', '/payout-institutions?currency=XOF&rail=MOBILE_MONEY', undefined);
    expect(missing.status).toBe(422);
    expect((missing.data.error as { code: string }).code).toBe('invalid_recipient');
    const withCountry = await call('GET', '/payout-institutions?currency=XOF&rail=MOBILE_MONEY&country=SN', undefined);
    expect(withCountry.status).toBe(200);
    expect(PayoutInstitutionSchema.array().safeParse(withCountry.data).success).toBe(true);
    expect((withCountry.data as unknown as unknown[]).length).toBeGreaterThan(0);
  });

  it('rejects requests without a valid token', async () => {
    const { status, data } = await call('POST', '/quotes', { source_currency: 'NGN', destination_currency: 'KES', amount: 1000, amount_side: 'source' }, 'bogus');
    expect(status).toBe(401);
    expect((data.error as { code: string }).code).toBe('unauthorized');
  });

  it('completes NGN to KES: bank details, transfer, Stellar settlement, payout', async () => {
    const token = await signup();
    const { quote, id, payment } = await kesPayment(token, 15_000_000, '0712345678');
    expect(QuoteSchema.safeParse(quote).success).toBe(true);
    expect(quote.fee_source_amount).toBe(150_000);
    expect(quote.send_amount).toBe(14_850_000);
    expect(payment.status).toBe('QUOTED');

    const pay = await call('POST', `/payments/${id}/pay`, {}, token);
    expect(pay.status).toBe(200);
    expect(DepositSchema.safeParse(pay.data).success).toBe(true);
    expect(pay.data.expected_amount).toBe(15_000_000);
    expect(String(pay.data.account_number)).toMatch(/^\d{10}$/);
    const again = await call('POST', `/payments/${id}/pay`, {}, token);
    expect(again.data.account_number).toBe(pay.data.account_number);

    const deposit = await call('POST', `/dev/payments/${id}/deposit`, {}, token);
    expect(deposit.data.outcome).toBe('processed');
    const { details, seen } = await waitFor(token, id, 'COMPLETED');
    expect(seen.has('AWAITING_PAYMENT') || seen.has('PAYMENT_DETECTED') || seen.has('PAYOUT_PENDING')).toBe(true);
    const kinds = (details.stellar_transactions as { kind: string; status: string }[]).map((t) => `${t.kind}:${t.status}`);
    expect(kinds).toEqual(['settle:succeeded', 'redeem:succeeded']);
    expect((details.deposit as { status: string }).status).toBe('received');
  });

  it('reverses on Stellar and refunds the naira when the payout fails', async () => {
    const token = await signup();
    const { id } = await kesPayment(token, 15_000_000, '0712340000');
    await call('POST', `/payments/${id}/pay`, {}, token);
    await call('POST', `/dev/payments/${id}/deposit`, {}, token);

    const { details } = await waitFor(token, id, 'REFUND_PENDING');
    const kinds = (details.stellar_transactions as { kind: string }[]).map((t) => t.kind);
    expect(kinds).toEqual(['settle', 'reverse']);
    const refund = details.refund as { status: string; amount: number; currency: string };
    expect(refund).toMatchObject({ status: 'submitted', amount: 15_000_000, currency: 'NGN' });

    const done = await waitFor(token, id, 'REFUNDED');
    expect((done.details.refund as { status: string }).status).toBe('completed');
  });

  it('lets the sender choose another account after a failed refund', async () => {
    const token = await signup();
    const { id } = await kesPayment(token, 15_000_000, '0712340000');
    await call('POST', `/payments/${id}/pay`, {}, token);
    await call('POST', `/dev/payments/${id}/deposit`, {}, token);
    await waitFor(token, id, 'REFUND_PENDING');
    expect((await call('POST', `/dev/payments/${id}/refund`, { status: 'failed' }, token)).status).toBe(200);
    const failed = await call('GET', `/payments/${id}`, undefined, token);
    expect((failed.data.refund as { status: string }).status).toBe('failed');

    const resolved = await call('POST', '/recipients/resolve', { currency: 'NGN', bank_code: '044', account_number: '1112223334' }, token);
    const retry = await call('POST', `/payments/${id}/refund`, { bank_code: '044', account_number: '1112223334', resolution_id: resolved.data.resolution_id }, token);
    expect(retry.status).toBe(200);
    expect(retry.data.status).toBe('submitted');
    await waitFor(token, id, 'REFUNDED');
  });

  it('rejects payment on a quote that has expired', async () => {
    const token = await signup();
    const q = await call('POST', '/quotes', { source_currency: 'NGN', destination_currency: 'KES', amount: 77_700, amount_side: 'source' }, token);
    await wait(80);
    const { status, data } = await call('POST', '/payments', { quote_id: q.data.id, recipient: { rail: 'KE_MOBILE_MONEY', phone_number: '0712345678' } }, token);
    expect(status).toBe(410);
    expect((data.error as { code: string }).code).toBe('quote_expired');
  });
});
