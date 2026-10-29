// Mock API for demos when the backend, LNbits or the network is unavailable.
// It answers the same routes with the same JSON shapes as backend/src/api, so the
// real Zod schemas validate its responses. Payment progress is derived from
// timestamps (not timers), so it survives page reloads.
//
// Magic values (demo only):
//   bank account ending 0000   payout fails; payment is refunded
//   bank account ending 1111   payout succeeds, then the provider reverses it
//   bank account ending 2222   the invoice is never paid and expires
//   bank account ending 9999   account resolution returns recipient_not_found
//   Lightning address starting "fail"   payout fails
//   Lightning address starting "expire" invoice expires
//   source amount 777          the quote expires after 5 seconds

import type { Currency, Rail } from './schemas';

const STORAGE_KEY = 'satsbridge.mock.v1';
const TIME_SCALE = Math.max(0.01, Number(process.env.NEXT_PUBLIC_MOCK_TIME_SCALE ?? '1') || 1);
const LATENCY_MS = 250 * TIME_SCALE;

const DETECT_S = 8;
const SETTLE_S = 9.5;
const PAYOUT_S = 11;
const OUTCOME_S = 15;
const REFUND_AFTER_S = 3;
const REVERSAL_AFTER_S = 7;
const QUOTE_TTL_S = 60;
const QUOTE_TTL_SHORT_S = 5;
const INVOICE_TTL_S = 60;
const STALL_INVOICE_TTL_S = 20;
const BTC_USD = 65_000;
const FEE_BPS = 150;

const EXPONENT: Record<Currency, number> = { XOF: 0, XAF: 0, GNF: 0, KES: 2, NGN: 2 };
// Approximate USD value of one major unit, for demo rates only.
const USD_PER_MAJOR: Record<Currency, number> = { XOF: 0.00166, XAF: 0.00166, GNF: 0.000116, KES: 0.0077, NGN: 0.00066 };

const MOBILE_OPERATORS = [
  { code: 'ORANGE', name: 'Orange Money', kind: 'mobile_money' },
  { code: 'MTN', name: 'MTN MoMo', kind: 'mobile_money' },
  { code: 'WAVE', name: 'Wave', kind: 'mobile_money' },
  { code: 'MOOV', name: 'Moov Money', kind: 'mobile_money' },
];

const BANKS = [
  { code: '058', name: 'GTBank', kind: 'bank' },
  { code: '044', name: 'Access Bank', kind: 'bank' },
  { code: '057', name: 'Zenith Bank', kind: 'bank' },
  { code: '033', name: 'United Bank for Africa', kind: 'bank' },
  { code: '011', name: 'First Bank of Nigeria', kind: 'bank' },
  { code: '999992', name: 'OPay', kind: 'wallet' },
] as const;

const NAMES = ['ADA OKAFOR', 'EMEKA NWOSU', 'FATIMA BELLO', 'TUNDE ADEYEMI', 'GRACE IBE', 'MUSA ABUBAKAR'];

interface MockUser {
  id: string;
  token: string;
  createdAt: string;
}

interface MockQuote {
  id: string;
  userId: string;
  source_currency: Currency;
  destination_currency: Currency;
  destination_rail: Rail;
  payout_amount_guaranteed: boolean;
  amount_side: 'source' | 'destination';
  source_amount: number;
  destination_amount: number;
  amount_sats: number;
  fee_sats: number;
  fee_source_amount: number;
  fee_bps: number;
  exchange_rate: string;
  rate_source: string;
  rate_as_of: string;
  expires_at: string;
  created_at: string;
  expires_at_ms: number;
}

interface MockResolution {
  id: string;
  userId: string;
  bank_code: string;
  account_number: string;
  account_name: string;
  expires_at: string;
}

interface MockRecipient {
  rail: Rail;
  bank_code?: string;
  account_number?: string;
  resolution_id?: string;
  address?: string;
  phone_number?: string;
  country?: string;
  provider_code?: string;
  account_name?: string;
}

interface MockPayment {
  id: string;
  userId: string;
  quote_id: string;
  source_currency: Currency;
  destination_currency: Currency;
  source_amount: number;
  expected_payout: number;
  amount_sats: number;
  fee_sats: number;
  exchange_rate: string;
  recipient_rail: Rail;
  recipient_masked: string;
  recipient_name: string | null;
  recipientKey: string;
  createdAtMs: number;
  expiresAtMs: number;
  invoice: { createdAtMs: number; expiresAtMs: number; payment_request: string; payment_hash: string } | null;
  devOutcome: { kind: 'success' | 'failed' | 'reversed'; atMs: number } | null;
  refund: { payment_hash: string; sats: number; atMs: number } | null;
}

interface MockDb {
  users: Record<string, MockUser>;
  quotes: Record<string, MockQuote>;
  resolutions: Record<string, MockResolution>;
  payments: Record<string, MockPayment>;
}

let db: MockDb | null = null;

function emptyDb(): MockDb {
  return { users: {}, quotes: {}, resolutions: {}, payments: {} };
}

function load(): MockDb {
  if (db) return db;
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null;
    db = raw ? (JSON.parse(raw) as MockDb) : emptyDb();
  } catch {
    db = emptyDb();
  }
  return db;
}

function save(): void {
  try {
    if (typeof localStorage !== 'undefined' && db) localStorage.setItem(STORAGE_KEY, JSON.stringify(db));
  } catch {
    // Storage unavailable: state lasts for this page session only.
  }
}

function randomId(prefix: string): string {
  const bytes = new Uint8Array(12);
  crypto.getRandomValues(bytes);
  return `${prefix}_${Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')}`;
}

function iso(ms: number): string {
  return new Date(ms).toISOString();
}

function scaled(seconds: number): number {
  return seconds * 1000 * TIME_SCALE;
}

function mask(value: string): string {
  return value.length <= 4 ? value : `${'*'.repeat(value.length - 4)}${value.slice(-4)}`;
}

function accountEnding(value: string): string {
  return value.slice(-4);
}

function mockError(status: number, code: string, message: string): Response {
  return json({ error: { code, message } }, status);
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

interface Derived {
  status: string;
  failure_reason: string | null;
  payout: null | Record<string, unknown> & { status: string };
  lightningStatus: 'pending' | 'settled';
  events: { id: number; payment_intent_id: string; event_type: string; from_status: null; to_status: string | null; detail: string | null; created_at: string }[];
}

function payoutOutcome(p: MockPayment): 'success' | 'failed' | 'reversed' | 'stall' {
  if (p.recipient_rail === 'LIGHTNING_ADDRESS') {
    if (p.recipientKey.startsWith('fail')) return 'failed';
    if (p.recipientKey.startsWith('expire')) return 'stall';
    return 'success';
  }
  const ending = accountEnding(p.recipientKey);
  if (ending === '0000') return 'failed';
  if (ending === '1111') return 'reversed';
  if (ending === '2222') return 'stall';
  return 'success';
}

function derive(p: MockPayment, nowMs: number): Derived {
  const events: Derived['events'] = [];
  let id = 1;
  const push = (eventType: string, to: string | null, atMs: number, detail: string | null = null) =>
    events.push({ id: id++, payment_intent_id: p.id, event_type: eventType, from_status: null, to_status: to, detail, created_at: iso(atMs) });

  push('payment.created', 'QUOTED', p.createdAtMs);
  const base = { status: 'QUOTED', failure_reason: null, payout: null, lightningStatus: 'pending' as const };

  if (!p.invoice) return { ...base, events };
  push('lightning.invoice_created', 'AWAITING_PAYMENT', p.invoice.createdAtMs);

  const outcome = payoutOutcome(p);
  const t = (nowMs - p.invoice.createdAtMs) / 1000 / TIME_SCALE;

  if (p.refund && nowMs >= p.refund.atMs) {
    const before = derive({ ...p, refund: null }, nowMs);
    const refundEvent = {
      id: before.events.length + 1,
      payment_intent_id: p.id,
      event_type: 'refund.completed',
      from_status: null,
      to_status: 'REFUNDED',
      detail: null,
      created_at: iso(p.refund.atMs),
    };
    return { ...before, status: 'REFUNDED', events: [...before.events, refundEvent] };
  }

  if (outcome === 'stall' && t < DETECT_S) {
    if (nowMs >= p.invoice.expiresAtMs) {
      push('lightning.expired', 'EXPIRED', p.invoice.expiresAtMs);
      return { ...base, status: 'EXPIRED', events };
    }
    return { ...base, status: 'AWAITING_PAYMENT', events };
  }
  if (outcome === 'stall' && t >= DETECT_S) {
    push('lightning.expired', 'EXPIRED', p.invoice.expiresAtMs);
    return { ...base, status: 'EXPIRED', events };
  }

  if (t < DETECT_S) return { ...base, status: 'AWAITING_PAYMENT', events };

  push('lightning.settled', 'PAYMENT_DETECTED', p.invoice.createdAtMs + scaled(DETECT_S));
  if (t < SETTLE_S) return { ...base, status: 'PAYMENT_DETECTED', lightningStatus: 'settled', events };
  push('payment.settled', 'SETTLED', p.invoice.createdAtMs + scaled(SETTLE_S));
  if (t < PAYOUT_S) return { ...base, status: 'SETTLED', lightningStatus: 'settled', events };
  push('payout.submitted', 'PAYOUT_PENDING', p.invoice.createdAtMs + scaled(PAYOUT_S));

  const dev = p.devOutcome;
  if (dev && nowMs >= dev.atMs) {
    const atMs = dev.atMs;
    if (dev.kind === 'success') {
      push('payout.completed', 'COMPLETED', atMs);
      return { ...base, status: 'COMPLETED', lightningStatus: 'settled', events, payout: payoutSnapshot(p, 'completed', null, atMs, null) };
    }
    push('payout.failed', 'FAILED', atMs, dev.kind === 'reversed' ? 'reversed by provider' : null);
    if (nowMs >= atMs + scaled(REFUND_AFTER_S)) push('refund.pending', 'REFUND_PENDING', atMs + scaled(REFUND_AFTER_S));
    return {
      ...base,
      status: nowMs >= atMs + scaled(REFUND_AFTER_S) ? 'REFUND_PENDING' : 'FAILED',
      lightningStatus: 'settled',
      events,
      failure_reason: dev.kind === 'reversed' ? 'Reversed by the payout provider' : 'The payout provider reported a failure',
      payout: payoutSnapshot(p, dev.kind === 'reversed' ? 'reversed' : 'failed', dev.kind === 'reversed' ? atMs : null, null, 'Payout not delivered'),
    };
  }

  const elapsedAfterPayout = t - PAYOUT_S;
  if (outcome === 'success' || elapsedAfterPayout < OUTCOME_S - PAYOUT_S) {
    if (outcome === 'success' && t >= OUTCOME_S) {
      push('payout.completed', 'COMPLETED', p.invoice.createdAtMs + scaled(OUTCOME_S));
      return { ...base, status: 'COMPLETED', lightningStatus: 'settled', events, payout: payoutSnapshot(p, 'completed', null, p.invoice.createdAtMs + scaled(OUTCOME_S), null) };
    }
    return { ...base, status: 'PAYOUT_PENDING', lightningStatus: 'settled', events, payout: payoutSnapshot(p, 'submitted', null, null, null) };
  }

  if (outcome === 'failed') {
    push('payout.failed', 'FAILED', p.invoice.createdAtMs + scaled(OUTCOME_S));
    if (t >= OUTCOME_S + REFUND_AFTER_S) push('refund.pending', 'REFUND_PENDING', p.invoice.createdAtMs + scaled(OUTCOME_S + REFUND_AFTER_S));
    return {
      ...base,
      status: t >= OUTCOME_S + REFUND_AFTER_S ? 'REFUND_PENDING' : 'FAILED',
      lightningStatus: 'settled',
      events,
      failure_reason: 'The bank rejected the transfer (mock)',
      payout: payoutSnapshot(p, 'failed', null, null, 'The bank rejected the transfer (mock)'),
    };
  }

  // outcome === 'reversed': completed first, then reversed, then refund pending.
  if (t < OUTCOME_S + REVERSAL_AFTER_S) {
    if (t >= OUTCOME_S) push('payout.completed', 'COMPLETED', p.invoice.createdAtMs + scaled(OUTCOME_S));
    return {
      ...base,
      status: t >= OUTCOME_S ? 'COMPLETED' : 'PAYOUT_PENDING',
      lightningStatus: 'settled',
      events,
      payout: payoutSnapshot(p, t >= OUTCOME_S ? 'completed' : 'submitted', null, t >= OUTCOME_S ? p.invoice.createdAtMs + scaled(OUTCOME_S) : null, null),
    };
  }
  push('payout.reversed', 'FAILED', p.invoice.createdAtMs + scaled(OUTCOME_S + REVERSAL_AFTER_S));
  const reversedAt = p.invoice.createdAtMs + scaled(OUTCOME_S + REVERSAL_AFTER_S);
  if (t >= OUTCOME_S + REVERSAL_AFTER_S + REFUND_AFTER_S) {
    push('refund.pending', 'REFUND_PENDING', p.invoice.createdAtMs + scaled(OUTCOME_S + REVERSAL_AFTER_S + REFUND_AFTER_S));
  }
  return {
    ...base,
    status: t >= OUTCOME_S + REVERSAL_AFTER_S + REFUND_AFTER_S ? 'REFUND_PENDING' : 'FAILED',
    lightningStatus: 'settled',
    events,
    failure_reason: 'Reversed by the payout provider after completion',
    payout: payoutSnapshot(p, 'reversed', reversedAt, p.invoice.createdAtMs + scaled(OUTCOME_S), 'Reversed by the payout provider'),
  };
}

function payoutSnapshot(
  p: MockPayment,
  status: string,
  reversedAtMs: number | null,
  completedAtMs: number | null,
  failureReason: string | null,
): NonNullable<Derived['payout']> {
  const provider = p.recipient_rail === 'NG_BANK_TRANSFER' ? 'mock-nip' : p.recipient_rail === 'KE_MOBILE_MONEY' ? 'mock-mpesa' : p.recipient_rail === 'MOBILE_MONEY' ? 'mock-momo' : 'mock-lnurl';
  return {
    payment_intent_id: p.id,
    provider,
    rail: p.recipient_rail,
    destination_currency: p.destination_currency,
    destination_amount: p.expected_payout,
    destination_reference: `SB${p.id.slice(-8).toUpperCase()}`,
    account_name: p.recipient_name,
    provider_reference: `mockref_${p.id.slice(-10)}`,
    status,
    failure_reason: failureReason,
    created_at: iso(p.createdAtMs),
    completed_at: completedAtMs ? iso(completedAtMs) : null,
    reversed_at: reversedAtMs ? iso(reversedAtMs) : null,
    needs_review: false,
    review_reason: null,
  };
}

function publicIntent(p: MockPayment, nowMs: number, d: Derived) {
  return {
    id: p.id,
    quote_id: p.quote_id,
    source_currency: p.source_currency,
    destination_currency: p.destination_currency,
    source_amount: p.source_amount,
    expected_payout: p.expected_payout,
    amount_sats: p.amount_sats,
    fee_sats: p.fee_sats,
    exchange_rate: p.exchange_rate,
    recipient_rail: p.recipient_rail,
    recipient_masked: p.recipient_masked,
    recipient_name: p.recipient_name,
    status: d.status,
    failure_reason: d.failure_reason,
    created_at: iso(p.createdAtMs),
    updated_at: iso(nowMs),
    expires_at: iso(p.expiresAtMs),
    refund_payment_hash: p.refund?.payment_hash ?? null,
    refund_sats: p.refund?.sats ?? null,
    refunded_at: p.refund ? iso(p.refund.atMs) : null,
  };
}

function details(p: MockPayment, nowMs: number) {
  const d = derive(p, nowMs);
  const lightning = p.invoice
    ? {
        payment_intent_id: p.id,
        payment_request: p.invoice.payment_request,
        payment_hash: p.invoice.payment_hash,
        amount_sats: p.amount_sats,
        status: d.lightningStatus,
        expires_at: iso(p.invoice.expiresAtMs),
        settled_at: d.lightningStatus === 'settled' ? iso(p.invoice.createdAtMs + scaled(DETECT_S)) : null,
        created_at: iso(p.invoice.createdAtMs),
      }
    : null;
  return {
    ...publicIntent(p, nowMs, d),
    lightning,
    lnurl: null,
    payout: d.payout,
    events: d.events,
  };
}

function bearer(token: string | undefined): MockUser | null {
  if (!token) return null;
  const user = Object.values(load().users).find((u) => u.token === token);
  return user ?? null;
}

function rateFor(source: Currency, destination: Currency): number {
  return USD_PER_MAJOR[source] / USD_PER_MAJOR[destination];
}

function currencyList() {
  const rails = {
    XOF: [{ rail: 'MOBILE_MONEY', provider: 'mock-momo', min_payout_minor: 500, max_payout_minor: 5_000_000, guaranteed_amount: true, countries: ['SN', 'CI', 'BJ', 'BF', 'ML', 'NE', 'TG', 'GW'] }],
    XAF: [{ rail: 'MOBILE_MONEY', provider: 'mock-momo', min_payout_minor: 500, max_payout_minor: 5_000_000, guaranteed_amount: true, countries: ['CM', 'CF', 'TD', 'CG', 'GQ', 'GA'] }],
    GNF: [{ rail: 'MOBILE_MONEY', provider: 'mock-momo', min_payout_minor: 5_000, max_payout_minor: 50_000_000, guaranteed_amount: true, countries: ['GN'] }],
    KES: [{ rail: 'KE_MOBILE_MONEY', provider: 'mock-mpesa', min_payout_minor: 100_00, max_payout_minor: 150_000_00, guaranteed_amount: true }],
    NGN: [
      { rail: 'NG_BANK_TRANSFER', provider: 'mock-nip', min_payout_minor: 100_00, max_payout_minor: 5_000_000_00, guaranteed_amount: true },
      { rail: 'LIGHTNING_ADDRESS', provider: 'mock-lnurl', min_payout_minor: null, max_payout_minor: null, guaranteed_amount: false },
    ],
  } as const;
  const names: Record<Currency, string> = {
    XOF: 'West African CFA franc',
    XAF: 'Central African CFA franc',
    GNF: 'Guinean franc',
    KES: 'Kenyan shilling',
    NGN: 'Nigerian naira',
  };
  return (Object.keys(EXPONENT) as Currency[]).map((code) => ({
    code,
    name: names[code],
    exponent: EXPONENT[code],
    payout_rails: rails[code],
  }));
}

function computeQuote(input: {
  source_currency: Currency;
  destination_currency: Currency;
  amount: number;
  amount_side: 'source' | 'destination';
  rail?: Rail;
}, nowMs: number, userId: string): MockQuote | Response {
  const { source_currency: src, destination_currency: dst, amount, amount_side: side } = input;
  const dstInfo = currencyList().find((c) => c.code === dst);
  const rail = dstInfo?.payout_rails.find((r) => r.rail === (input.rail ?? dstInfo.payout_rails[0]?.rail));
  if (!dstInfo || !rail) return mockError(400, 'validation_error', 'Unsupported payout method.');
  if (!Number.isSafeInteger(amount) || amount <= 0) return mockError(400, 'validation_error', 'Amount must be positive.');

  const rate = rateFor(src, dst);
  const srcExp = EXPONENT[src];
  const dstExp = EXPONENT[dst];
  const perMinor = rate * 10 ** (dstExp - srcExp);
  let sourceAmount: number;
  let fee: number;
  let destAmount: number;
  if (side === 'source') {
    sourceAmount = amount;
    fee = Math.ceil((amount * FEE_BPS) / 10_000);
    destAmount = Math.floor((amount - fee) * perMinor);
  } else {
    destAmount = amount;
    const net = Math.ceil(amount / perMinor);
    fee = Math.ceil((net * FEE_BPS) / (10_000 - FEE_BPS));
    sourceAmount = net + fee;
  }
  const usdPerSourceMinor = USD_PER_MAJOR[src] / 10 ** srcExp;
  const amountSats = Math.ceil(((sourceAmount * usdPerSourceMinor) / BTC_USD) * 1e8);
  const feeSats = Math.ceil(((fee * usdPerSourceMinor) / BTC_USD) * 1e8);
  const ttl = sourceAmount === 777 * 10 ** srcExp ? QUOTE_TTL_SHORT_S : QUOTE_TTL_S;
  const expiresAtMs = nowMs + scaled(ttl);

  return {
    id: randomId('qt'),
    userId,
    source_currency: src,
    destination_currency: dst,
    destination_rail: rail.rail,
    payout_amount_guaranteed: rail.guaranteed_amount,
    amount_side: side,
    source_amount: sourceAmount,
    destination_amount: destAmount,
    amount_sats: amountSats,
    fee_sats: feeSats,
    fee_source_amount: fee,
    fee_bps: FEE_BPS,
    exchange_rate: rate.toFixed(4),
    rate_source: 'Coinbase (mock)',
    rate_as_of: iso(nowMs - 3000),
    expires_at: iso(expiresAtMs),
    created_at: iso(nowMs),
    expires_at_ms: expiresAtMs,
  };
}

function publicQuote(q: MockQuote) {
  return {
    id: q.id,
    source_currency: q.source_currency,
    destination_currency: q.destination_currency,
    destination_rail: q.destination_rail,
    payout_amount_guaranteed: q.payout_amount_guaranteed,
    amount_side: q.amount_side,
    source_amount: q.source_amount,
    destination_amount: q.destination_amount,
    amount_sats: q.amount_sats,
    fee_sats: q.fee_sats,
    fee_source_amount: q.fee_source_amount,
    fee_bps: q.fee_bps,
    exchange_rate: q.exchange_rate,
    rate_source: q.rate_source,
    rate_as_of: q.rate_as_of,
    expires_at: q.expires_at,
    created_at: q.created_at,
  };
}

function recipientKey(r: MockRecipient): string {
  if (r.rail === 'NG_BANK_TRANSFER') return r.account_number ?? '';
  if (r.rail === 'KE_MOBILE_MONEY' || r.rail === 'MOBILE_MONEY') return r.phone_number ?? '';
  if (r.rail === 'LIGHTNING_ADDRESS') return (r.address ?? '').toLowerCase();
  return '';
}

function maskedRecipient(r: MockRecipient): string {
  if (r.rail === 'NG_BANK_TRANSFER') return `${r.bank_code}:${mask(r.account_number ?? '')}`;
  if (r.rail === 'KE_MOBILE_MONEY') return mask(r.phone_number ?? '');
  if (r.rail === 'MOBILE_MONEY') return `${(r.country ?? '').toUpperCase()}:${r.provider_code ?? ''}:${mask(r.phone_number ?? '')}`;
  return (r.address ?? '').toLowerCase();
}

function invoiceFor(p: MockPayment, nowMs: number): NonNullable<MockPayment['invoice']> {
  const outcome = payoutOutcome(p);
  const ttl = outcome === 'stall' ? STALL_INVOICE_TTL_S : INVOICE_TTL_S;
  const sats = p.amount_sats;
  const hash = randomId('h').replace(/_/g, '').slice(0, 64);
  // BOLT11 amounts: "n" is 100 msat, so sats * 10 n = sats * 1000 msat.
  return {
    createdAtMs: nowMs,
    expiresAtMs: nowMs + scaled(ttl),
    payment_request: `lnbc${sats * 10}n1mock${p.id.slice(-12)}`,
    payment_hash: hash,
  };
}

// Decodes the amount of a BOLT11 invoice in msat, for refund validation.
function invoiceMsat(invoice: string): number | null {
  const match = /^ln(?:bc|tb|bcrt)(\d+)([munp])?1/i.exec(invoice.trim());
  if (!match) return null;
  const amount = Number(match[1]);
  const multiplier: Record<string, number> = { m: 1e8, u: 1e5, n: 100, p: 0.1 };
  const msat = match[2] ? amount * multiplier[match[2].toLowerCase()] : amount * 1e11;
  return Number.isFinite(msat) ? Math.round(msat) : null;
}

function routeMatch(path: string): { name: string; id?: string } | null {
  const url = path.split('?')[0];
  const parts = url.split('/').filter(Boolean);
  if (url === '/health') return { name: 'health' };
  if (url === '/currencies') return { name: 'currencies' };
  if (url === '/payout-institutions') return { name: 'institutions' };
  if (url === '/users') return { name: 'users' };
  if (url === '/quotes') return { name: 'quote-create' };
  if (parts[0] === 'quotes' && parts.length === 2) return { name: 'quote-get', id: parts[1] };
  if (url === '/recipients/resolve') return { name: 'resolve' };
  if (url === '/payments') return { name: 'payment-create' };
  if (url === '/transactions') return { name: 'transactions' };
  if (parts[0] === 'payments' && parts.length === 2) return { name: 'payment-get', id: parts[1] };
  if (parts[0] === 'payments' && parts.length === 3 && parts[2] === 'pay') return { name: 'payment-pay', id: parts[1] };
  if (parts[0] === 'payments' && parts.length === 3 && parts[2] === 'refund') return { name: 'payment-refund', id: parts[1] };
  if (parts[0] === 'dev' && parts[1] === 'payments' && parts.length === 4) return { name: 'dev-payout', id: parts[2] };
  return null;
}

export async function mockRespond(method: string, path: string, body: unknown, token: string | undefined): Promise<Response> {
  await new Promise((resolve) => setTimeout(resolve, LATENCY_MS));
  const nowMs = Date.now();
  const route = routeMatch(path);
  const url = new URL(path, 'http://mock.local');
  const state = load();
  const payload = (body ?? {}) as Record<string, unknown>;

  if (!route) return mockError(404, 'not_found', 'Route not found.');

  if (route.name === 'health') return json({ status: 'ok', version: 'mock', lightning: 'mock', lnurl: false });
  if (route.name === 'currencies') return json(currencyList());
  if (route.name === 'institutions') {
    const currency = url.searchParams.get('currency');
    if (currency === 'XOF' || currency === 'XAF') return json(MOBILE_OPERATORS.map((o) => ({ ...o })));
    if (currency !== 'NGN') return json([]);
    return json(BANKS.map((b) => ({ ...b })));
  }

  if (route.name === 'users' && method === 'POST') {
    const user: MockUser = { id: randomId('usr'), token: randomId('sbu_mock'), createdAt: iso(nowMs) };
    state.users[user.id] = user;
    save();
    return json({ user_id: user.id, token: user.token, created_at: user.createdAt }, 201);
  }

  const user = bearer(token);
  if (!user) return mockError(401, 'unauthorized', 'Invalid or missing token.');

  if (route.name === 'quote-create') {
    const quote = computeQuote(payload as Parameters<typeof computeQuote>[0], nowMs, user.id);
    if (quote instanceof Response) return quote;
    state.quotes[quote.id] = quote;
    save();
    return json(publicQuote(quote));
  }

  if (route.name === 'quote-get') {
    const quote = state.quotes[route.id ?? ''];
    if (!quote || quote.userId !== user.id) return mockError(404, 'not_found', 'Quote not found.');
    return json(publicQuote(quote));
  }

  if (route.name === 'resolve') {
    const bankCode = String(payload.bank_code ?? '');
    const accountNumber = String(payload.account_number ?? '');
    if (!BANKS.some((b) => b.code === bankCode)) return mockError(400, 'validation_error', 'Unknown bank.');
    if (!/^\d{10}$/.test(accountNumber)) return mockError(400, 'validation_error', 'Account number must be 10 digits.');
    if (accountEnding(accountNumber) === '9999') return mockError(422, 'recipient_not_found', 'Account not found.');
    const id = randomId('res');
    const resolution: MockResolution = {
      id,
      userId: user.id,
      bank_code: bankCode,
      account_number: accountNumber,
      account_name: NAMES[Number(accountNumber[9]) % NAMES.length],
      expires_at: iso(nowMs + scaled(600)),
    };
    state.resolutions[id] = resolution;
    save();
    return json({
      resolution_id: id,
      account_name: resolution.account_name,
      bank_code: bankCode,
      account_number_masked: mask(accountNumber),
      expires_at: resolution.expires_at,
    });
  }

  if (route.name === 'payment-create') {
    const quote = state.quotes[String(payload.quote_id ?? '')];
    if (!quote || quote.userId !== user.id) return mockError(404, 'not_found', 'Quote not found.');
    if (nowMs >= quote.expires_at_ms) return mockError(410, 'quote_expired', 'Quote expired.');
    const recipient = payload.recipient as MockRecipient | undefined;
    if (!recipient || recipient.rail !== quote.destination_rail) {
      return mockError(400, 'validation_error', 'Recipient does not match the quote.');
    }
    let recipientName: string | null = null;
    if (recipient.rail === 'NG_BANK_TRANSFER') {
      const resolution = state.resolutions[recipient.resolution_id as string];
      if (
        !resolution ||
        resolution.userId !== user.id ||
        resolution.bank_code !== recipient.bank_code ||
        resolution.account_number !== recipient.account_number
      ) {
        return mockError(422, 'invalid_recipient', 'Verify the account again before paying.');
      }
      recipientName = resolution.account_name;
    }
    if (recipient.rail === 'MOBILE_MONEY') {
      if (!/^\d{8,12}$/.test(recipient.phone_number ?? '') || !recipient.country || !recipient.provider_code) {
        return mockError(422, 'invalid_recipient', 'Mobile money details are not valid.');
      }
      recipientName = recipient.account_name?.trim() || null;
    }
    if (recipient.rail === 'LIGHTNING_ADDRESS' && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(recipient.address ?? '')) {
      return mockError(422, 'invalid_recipient', 'Lightning Address is not valid.');
    }
    const id = randomId('pay');
    const payment: MockPayment = {
      id,
      userId: user.id,
      quote_id: quote.id,
      source_currency: quote.source_currency,
      destination_currency: quote.destination_currency,
      source_amount: quote.source_amount,
      expected_payout: quote.destination_amount,
      amount_sats: quote.amount_sats,
      fee_sats: quote.fee_sats,
      exchange_rate: quote.exchange_rate,
      recipient_rail: recipient.rail,
      recipient_masked: maskedRecipient(recipient),
      recipient_name: recipientName,
      recipientKey: recipientKey(recipient),
      createdAtMs: nowMs,
      expiresAtMs: nowMs + scaled(3600),
      invoice: null,
      devOutcome: null,
      refund: null,
    };
    state.payments[id] = payment;
    save();
    return json(publicIntent(payment, nowMs, derive(payment, nowMs)), 201);
  }

  const payment = route.id ? state.payments[route.id] : undefined;
  if (route.name.startsWith('payment-') || route.name === 'dev-payout') {
    if (!payment || payment.userId !== user.id) return mockError(404, 'not_found', 'Payment not found.');
  }

  if (route.name === 'payment-get' && payment) return json(details(payment, nowMs));

  if (route.name === 'payment-pay' && payment) {
    if (!payment.invoice) {
      payment.invoice = invoiceFor(payment, nowMs);
      save();
    }
    const invoice = payment.invoice;
    const d = derive(payment, nowMs);
    if (d.status === 'EXPIRED') return mockError(409, 'conflict', 'This invoice has expired.');
    return json({
      payment_intent_id: payment.id,
      payment_request: invoice.payment_request,
      payment_hash: invoice.payment_hash,
      amount_sats: payment.amount_sats,
      status: d.lightningStatus,
      expires_at: iso(invoice.expiresAtMs),
      settled_at: d.lightningStatus === 'settled' ? iso(invoice.createdAtMs + scaled(DETECT_S)) : null,
      created_at: iso(invoice.createdAtMs),
      lnurl: null,
    });
  }

  if (route.name === 'payment-refund' && payment) {
    const d = derive(payment, nowMs);
    if (d.status !== 'REFUND_PENDING') {
      return mockError(409, 'conflict', `payment is ${d.status}; nothing to refund`);
    }
    const invoice = String(payload.payment_request ?? '');
    const msat = invoiceMsat(invoice);
    if (msat === null) return mockError(400, 'validation_error', 'Not a valid Lightning invoice.');
    if (msat !== payment.amount_sats * 1000) {
      return mockError(400, 'validation_error', `the refund invoice must be for exactly ${payment.amount_sats} sats`);
    }
    payment.refund = { payment_hash: randomId('rfh').slice(0, 40), sats: payment.amount_sats, atMs: nowMs };
    save();
    return json(publicIntent(payment, nowMs, derive(payment, nowMs)));
  }

  if (route.name === 'payment-refund' && !payment) return mockError(404, 'not_found', 'Payment not found.');

  if (route.name === 'transactions') {
    const limit = Math.min(200, Math.max(1, Number(url.searchParams.get('limit') ?? '50') || 50));
    const list = Object.values(state.payments)
      .filter((p) => p.userId === user.id)
      .sort((a, b) => b.createdAtMs - a.createdAtMs)
      .slice(0, limit)
      .map((p) => publicIntent(p, nowMs, derive(p, nowMs)));
    return json(list);
  }

  if (route.name === 'dev-payout' && payment) {
    const status = String(payload.status ?? '');
    if (status !== 'success' && status !== 'failed' && status !== 'reversed') {
      return mockError(400, 'validation_error', 'status must be success, failed or reversed.');
    }
    const d = derive(payment, nowMs);
    if (d.status !== 'PAYOUT_PENDING') return mockError(409, 'conflict', 'Payout is not pending.');
    payment.devOutcome = { kind: status, atMs: nowMs };
    save();
    return json({ outcome: 'processed' });
  }

  return mockError(404, 'not_found', 'Route not found.');
}

export function isMockApi(): boolean {
  return process.env.NEXT_PUBLIC_MOCK_API === 'true';
}
