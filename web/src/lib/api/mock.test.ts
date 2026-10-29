import { beforeAll, describe, expect, it } from 'vitest';
import { CurrencyInfoSchema, PayoutInstitutionSchema, PaymentDetailsSchema, PaymentIntentSchema, PayResponseSchema, QuoteSchema, RecipientResolutionSchema, UserSchema } from './schemas';

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

async function quote(token: string, amount: number) {
  const { status, data } = await call(
    'POST',
    '/quotes',
    { source_currency: 'XOF', destination_currency: 'NGN', amount, amount_side: 'source', rail: 'NG_BANK_TRANSFER' },
    token,
  );
  expect(status).toBe(200);
  return data;
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
    const resolved = await call('POST', '/recipients/resolve', { currency: 'NGN', bank_code: '058', account_number: '0123456789' }, user.data.token as string);
    expect(RecipientResolutionSchema.safeParse(resolved.data).success).toBe(true);
  });

  it('accepts a mobile money recipient for XOF and rejects a malformed number', async () => {
    const token = await signup();
    const { data: q } = await call('POST', '/quotes', { source_currency: 'NGN', destination_currency: 'XOF', amount: 500000, amount_side: 'source', rail: 'MOBILE_MONEY' }, token);
    const recipient = { rail: 'MOBILE_MONEY', country: 'SN', provider_code: 'WAVE', phone_number: '771234567' };
    const bad = await call('POST', '/payments', { quote_id: q.id, recipient: { ...recipient, phone_number: '12' } }, token);
    expect(bad.status).toBe(422);
    const ok = await call('POST', '/payments', { quote_id: q.id, recipient }, token);
    expect(ok.status).toBe(201);
    expect(PaymentIntentSchema.safeParse(ok.data).success).toBe(true);
    expect(ok.data.recipient_masked).toBe('SN:WAVE:*****4567');
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
    const { status, data } = await call('POST', '/quotes', { source_currency: 'XOF', destination_currency: 'NGN', amount: 1000, amount_side: 'source' }, 'bogus');
    expect(status).toBe(401);
    expect((data.error as { code: string }).code).toBe('unauthorized');
  });

  it('completes the XOF to NGN happy path through every status', async () => {
    const token = await signup();
    const q = await quote(token, 10_000);
    expect(q.payout_amount_guaranteed).toBe(true);
    expect(q.source_currency).toBe('XOF');
    const created = await call('POST', '/payments', {
      quote_id: q.id,
      recipient: { rail: 'NG_BANK_TRANSFER', bank_code: '058', account_number: '0123456789', resolution_id: 'missing' },
    }, token);
    expect(created.status).toBe(422);

    const resolved = await call('POST', '/recipients/resolve', { currency: 'NGN', bank_code: '058', account_number: '0123456789' }, token);
    const payment = await call('POST', '/payments', {
      quote_id: q.id,
      recipient: { rail: 'NG_BANK_TRANSFER', bank_code: '058', account_number: '0123456789', resolution_id: resolved.data.resolution_id },
    }, token);
    expect(payment.status).toBe(201);
    const id = payment.data.id as string;
    expect(payment.data.status).toBe('QUOTED');

    expect(QuoteSchema.safeParse(q).success).toBe(true);
    expect(PaymentIntentSchema.safeParse(payment.data).success).toBe(true);

    const pay = await call('POST', `/payments/${id}/pay`, {}, token);
    expect(pay.status).toBe(200);
    expect(pay.data.amount_sats).toBe(q.amount_sats);
    expect(PayResponseSchema.safeParse(pay.data).success).toBe(true);

    const seen = new Set<string>();
    for (let i = 0; i < 60; i++) {
      const details = await call('GET', `/payments/${id}`, undefined, token);
      seen.add(details.data.status as string);
      expect(PaymentDetailsSchema.safeParse(details.data).success).toBe(true);
      if (details.data.status === 'COMPLETED') break;
      await wait(20);
    }
    expect(seen.has('COMPLETED')).toBe(true);
    expect(seen.has('AWAITING_PAYMENT')).toBe(true);
  });

  it('moves a failed payout to REFUND_PENDING and refunds only the exact amount', async () => {
    const token = await signup();
    const q = await quote(token, 10_000);
    const payment = await call('POST', '/payments', {
      quote_id: q.id,
      recipient: { rail: 'NG_BANK_TRANSFER', bank_code: '058', account_number: '0000000000', resolution_id: (await call('POST', '/recipients/resolve', { currency: 'NGN', bank_code: '058', account_number: '0000000000' }, token)).data.resolution_id },
    }, token);
    const id = payment.data.id as string;
    await call('POST', `/payments/${id}/pay`, {}, token);

    let status = '';
    for (let i = 0; i < 200 && status !== 'REFUND_PENDING'; i++) {
      status = (await call('GET', `/payments/${id}`, undefined, token)).data.status as string;
      await wait(20);
    }
    expect(status).toBe('REFUND_PENDING');

    const sats = q.amount_sats as number;
    const wrong = await call('POST', `/payments/${id}/refund`, { payment_request: `lnbc${sats * 10 + 1}n1mock` }, token);
    expect(wrong.status).toBe(400);
    expect(String((wrong.data.error as { message: string }).message)).toContain(`exactly ${sats} sats`);

    const right = await call('POST', `/payments/${id}/refund`, { payment_request: `lnbc${sats * 10}n1mock` }, token);
    expect(right.status).toBe(200);
    expect(right.data.status).toBe('REFUNDED');
    expect(right.data.refund_sats).toBe(sats);
    expect(right.data.refunded_at).not.toBeNull();
    expect(PaymentIntentSchema.safeParse(right.data).success).toBe(true);
    const after = await call('GET', `/payments/${id}`, undefined, token);
    expect(PaymentDetailsSchema.safeParse(after.data).success).toBe(true);
    expect(after.data.status).toBe('REFUNDED');
  });

  it('rejects payment on a quote that has expired', async () => {
    const token = await signup();
    const q = await quote(token, 777);
    await wait(80);
    const { status, data } = await call('POST', '/payments', {
      quote_id: q.id,
      recipient: { rail: 'NG_BANK_TRANSFER', bank_code: '058', account_number: '0123456789', resolution_id: 'x' },
    }, token);
    expect(status).toBe(410);
    expect((data.error as { code: string }).code).toBe('quote_expired');
  });
});
