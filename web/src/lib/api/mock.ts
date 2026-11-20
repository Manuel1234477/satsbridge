// Mock API for demos when the backend or the network is unavailable.
// It answers the same routes with the same JSON shapes as backend/src/api, so the
// real Zod schemas validate its responses. Payment progress is derived from
// timestamps (not timers), so it survives page reloads.
//
// Flow: NGN quote -> payment -> bank transfer details -> transfer "arrives" after a few seconds
// (or at once via the dev deposit route) -> settled on a simulated Stellar ledger -> payout.
//
// Magic values (demo only), on the recipient's phone or account number:
//   ending 0000   payout fails; the conversion is reversed and the sender refunded
//   ending 1111   payout succeeds, then the provider reverses it; refund follows
//   ending 2222   the sender never transfers; the payment expires
//   NGN account ending 9999   account resolution returns recipient_not_found
//   source amount ₦777        the quote expires after 5 seconds

import type { Currency, Rail } from './schemas';

const STORAGE_KEY = 'satsbridge.mock.v2';
const TIME_SCALE = Math.max(0.01, Number(process.env.NEXT_PUBLIC_MOCK_TIME_SCALE ?? '1') || 1);
const LATENCY_MS = 250 * TIME_SCALE;

// Seconds after the bank details are issued, or after the deposit.
const AUTO_DEPOSIT_S = 8;
const SETTLE_S = 1.5;
const PAYOUT_S = 3;
const OUTCOME_S = 7;
const REVERSAL_AFTER_S = 7;
const REFUND_DONE_S = 4;
const QUOTE_TTL_S = 60;
const QUOTE_TTL_SHORT_S = 5;
const DEPOSIT_TTL_S = 30 * 60;
const STALL_DEPOSIT_TTL_S = 20;
const FEE_BPS = 100;
const SLIPPAGE_BPS = 50;
const SPREAD_BPS = 30;

const EXPONENT: Record<Currency, number> = { XOF: 0, XAF: 0, GNF: 0, KES: 2, NGN: 2, GHS: 2 };
// Units per US dollar, for demo rates only.
const PER_USD: Record<Currency, number> = { NGN: 1550, KES: 129, GHS: 15.4, XOF: 603.5, XAF: 603.5, GNF: 8650 };
const NAMES: Record<Currency, string> = {
  XOF: 'West African CFA franc',
  XAF: 'Central African CFA franc',
  GNF: 'Guinean franc',
  KES: 'Kenyan shilling',
  NGN: 'Nigerian naira',
  GHS: 'Ghanaian cedi',
};
const MOCK_ISSUER = 'GMOCKISSUERXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX';

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

const ACCOUNT_NAMES = ['ADA OKAFOR', 'EMEKA NWOSU', 'FATIMA BELLO', 'TUNDE ADEYEMI', 'GRACE IBE', 'MUSA ABUBAKAR'];
const DEMO_SENDER = { bank_code: '058', account_number: '0123456789', account_name: 'DEMO SENDER' };

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
  fee_source_amount: number;
  fee_bps: number;
  send_amount: number;
  destination_amount: number;
  exchange_rate: string;
  rate_source: string;
  conversion_path: string[];
  rate_as_of: string;
  expires_at: string;
  created_at: string;
  expires_at_ms: number;
  delivered_stroops: number;
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
  fee_source_amount: number;
  send_amount: number;
  expected_payout: number;
  exchange_rate: string;
  conversion_path: string[];
  delivered_stroops: number;
  recipient_rail: Rail;
  recipient_masked: string;
  recipient_name: string | null;
  recipientKey: string;
  createdAtMs: number;
  expiresAtMs: number;
  deposit: { createdAtMs: number; expiresAtMs: number; accountNumber: string; receivedAtMs: number | null } | null;
  devOutcome: { kind: 'success' | 'failed' | 'reversed'; atMs: number } | null;
  refundOutcome: { kind: 'success' | 'failed'; atMs: number } | null;
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

function randomHex(bytes: number): string {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return Array.from(buf, (b) => b.toString(16).padStart(2, '0')).join('');
}

function randomId(prefix: string): string {
  return `${prefix}_${randomHex(12)}`;
}

// A stable fake transaction hash per payment and step.
function txHash(paymentId: string, kind: string): string {
  let h = 0;
  const seed = `${paymentId}:${kind}`;
  let out = '';
  for (let round = 0; out.length < 64; round++) {
    for (let i = 0; i < seed.length; i++) h = (Math.imul(h ^ seed.charCodeAt(i), 2654435761) + round) >>> 0;
    out += h.toString(16).padStart(8, '0');
  }
  return out.slice(0, 64);
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

function mockError(status: number, code: string, message: string): Response {
  return json({ error: { code, message } }, status);
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

type Outcome = 'success' | 'failed' | 'reversed' | 'stall';

function payoutOutcome(p: MockPayment): Outcome {
  const ending = p.recipientKey.slice(-4);
  if (ending === '0000') return 'failed';
  if (ending === '1111') return 'reversed';
  if (ending === '2222') return 'stall';
  return 'success';
}

interface Event {
  id: number;
  payment_intent_id: string;
  event_type: string;
  from_status: null;
  to_status: string | null;
  detail: string | null;
  created_at: string;
}

interface StellarStep {
  kind: 'settle' | 'redeem' | 'reverse';
  atMs: number;
}

interface Derived {
  status: string;
  failure_reason: string | null;
  depositStatus: 'awaiting' | 'received' | 'expired';
  receivedAtMs: number | null;
  payout: null | { status: string; completedAtMs: number | null; reversedAtMs: number | null; failureReason: string | null };
  stellar: StellarStep[];
  refund: null | { status: 'submitted' | 'completed' | 'failed'; atMs: number; doneAtMs: number | null };
  events: Event[];
}

function derive(p: MockPayment, nowMs: number): Derived {
  const events: Event[] = [];
  const push = (eventType: string, to: string | null, atMs: number, detail: string | null = null) =>
    events.push({ id: events.length + 1, payment_intent_id: p.id, event_type: eventType, from_status: null, to_status: to, detail, created_at: iso(atMs) });
  const base: Derived = { status: 'QUOTED', failure_reason: null, depositStatus: 'awaiting', receivedAtMs: null, payout: null, stellar: [], refund: null, events };

  push('payment.created', 'QUOTED', p.createdAtMs);
  if (!p.deposit) return base;
  push('deposit.instructions_issued', 'AWAITING_PAYMENT', p.deposit.createdAtMs, p.deposit.accountNumber);

  const outcome = payoutOutcome(p);
  const autoAt = outcome === 'stall' ? null : p.deposit.createdAtMs + scaled(AUTO_DEPOSIT_S);
  const receivedAt = p.deposit.receivedAtMs ?? autoAt;
  if (receivedAt === null || nowMs < receivedAt) {
    if (nowMs >= p.deposit.expiresAtMs) {
      push('deposit.expired', 'EXPIRED', p.deposit.expiresAtMs);
      return { ...base, status: 'EXPIRED', depositStatus: 'expired' };
    }
    return { ...base, status: 'AWAITING_PAYMENT' };
  }
  const at = (s: number) => receivedAt + scaled(s);
  const after = (s: number) => nowMs >= at(s);
  const received = { ...base, depositStatus: 'received' as const, receivedAtMs: receivedAt };

  push('deposit.received', 'PAYMENT_DETECTED', receivedAt, `${p.source_amount} NGN`);
  if (!after(SETTLE_S)) return { ...received, status: 'PAYMENT_DETECTED', stellar: [{ kind: 'settle', atMs: receivedAt }] };
  const settle: StellarStep = { kind: 'settle', atMs: at(SETTLE_S) };
  push('stellar.settled', 'SETTLED', at(SETTLE_S), txHash(p.id, 'settle'));
  if (!after(PAYOUT_S)) return { ...received, status: 'SETTLED', stellar: [settle] };
  push('payout.requested', 'PAYOUT_PENDING', at(PAYOUT_S));
  const pending = { ...received, stellar: [settle] };

  // A dev simulation overrides the magic-value outcome.
  const dev = p.devOutcome;
  const kind = dev ? dev.kind : outcome === 'stall' ? 'success' : outcome;
  const outcomeAt = dev ? dev.atMs : at(OUTCOME_S);
  if (nowMs < outcomeAt) {
    return { ...pending, status: 'PAYOUT_PENDING', payout: { status: 'submitted', completedAtMs: null, reversedAtMs: null, failureReason: null } };
  }

  const refunding = (failedAt: number, reason: string, payout: Derived['payout'], extra: StellarStep[]): Derived => {
    push('refund.pending', 'REFUND_PENDING', failedAt);
    const stellar = [...pending.stellar, ...extra];
    const doneAt = p.refundOutcome ? p.refundOutcome.atMs : failedAt + scaled(REFUND_DONE_S);
    if (nowMs < doneAt) {
      return { ...pending, status: 'REFUND_PENDING', failure_reason: reason, payout, stellar, refund: { status: 'submitted', atMs: failedAt, doneAtMs: null } };
    }
    if (p.refundOutcome?.kind === 'failed') {
      return { ...pending, status: 'REFUND_PENDING', failure_reason: reason, payout, stellar, refund: { status: 'failed', atMs: failedAt, doneAtMs: doneAt } };
    }
    push('refund.completed', 'REFUNDED', doneAt);
    return { ...pending, status: 'REFUNDED', failure_reason: reason, payout, stellar, refund: { status: 'completed', atMs: failedAt, doneAtMs: doneAt } };
  };

  if (kind === 'failed') {
    const reason = 'The payout provider rejected the transfer (mock)';
    push('payout.failed', 'FAILED', outcomeAt, reason);
    return refunding(outcomeAt, reason, { status: 'failed', completedAtMs: null, reversedAtMs: null, failureReason: reason }, [
      { kind: 'reverse', atMs: outcomeAt },
    ]);
  }

  push('payout.completed', 'COMPLETED', outcomeAt);
  const redeem: StellarStep = { kind: 'redeem', atMs: outcomeAt };
  const reversalAt = kind === 'reversed' ? (dev ? outcomeAt : outcomeAt + scaled(REVERSAL_AFTER_S)) : null;
  if (reversalAt === null || nowMs < reversalAt) {
    return {
      ...pending,
      status: 'COMPLETED',
      stellar: [settle, redeem],
      payout: { status: 'completed', completedAtMs: outcomeAt, reversedAtMs: null, failureReason: null },
    };
  }
  const reason = 'Reversed by the payout provider after completion';
  push('payout.reversed', 'FAILED', reversalAt, reason);
  return refunding(reversalAt, reason, { status: 'reversed', completedAtMs: outcomeAt, reversedAtMs: reversalAt, failureReason: reason }, [redeem]);
}

function stellarTx(p: MockPayment, step: StellarStep, nowMs: number, settled: boolean) {
  const confirmed = step.kind !== 'settle' || settled;
  const stroops = (minor: number) => minor * 10 ** (7 - EXPONENT[p.source_currency]);
  const operation =
    step.kind === 'settle'
      ? { kind: 'settle', source: p.source_currency, destination: p.destination_currency, mint: stroops(p.source_amount), send: stroops(p.send_amount), dest_min: p.expected_payout * 10 ** (7 - EXPONENT[p.destination_currency]) }
      : step.kind === 'redeem'
        ? { kind: 'redeem', currency: p.destination_currency, amount: p.delivered_stroops }
        : { kind: 'reverse', from: p.destination_currency, to: p.source_currency, amount: p.delivered_stroops, burn: stroops(p.fee_source_amount) };
  return {
    kind: step.kind,
    tx_hash: txHash(p.id, step.kind),
    operation,
    status: confirmed ? 'succeeded' : 'submitted',
    delivered: step.kind === 'settle' && confirmed ? p.delivered_stroops : null,
    ledger: confirmed ? 1_000 + Math.floor(step.atMs / 5000) % 100_000 : null,
    error: null,
    valid_until: iso(step.atMs + 60_000),
    created_at: iso(step.atMs),
    confirmed_at: confirmed ? iso(Math.min(nowMs, step.atMs)) : null,
    explorer_url: null,
  };
}

function publicIntent(p: MockPayment, nowMs: number, d: Derived) {
  return {
    id: p.id,
    quote_id: p.quote_id,
    source_currency: p.source_currency,
    destination_currency: p.destination_currency,
    source_amount: p.source_amount,
    fee_source_amount: p.fee_source_amount,
    send_amount: p.send_amount,
    expected_payout: p.expected_payout,
    exchange_rate: p.exchange_rate,
    conversion_path: p.conversion_path,
    recipient_rail: p.recipient_rail,
    recipient_masked: p.recipient_masked,
    recipient_name: p.recipient_name,
    status: d.status,
    failure_reason: d.failure_reason,
    created_at: iso(p.createdAtMs),
    updated_at: iso(nowMs),
    expires_at: iso(p.deposit?.expiresAtMs ?? p.expiresAtMs),
  };
}

function depositView(p: MockPayment, d: Derived) {
  if (!p.deposit) return null;
  return {
    payment_intent_id: p.id,
    provider: 'ngn-collection-mock',
    currency: p.source_currency,
    expected_amount: p.source_amount,
    reference: p.deposit.accountNumber,
    bank_name: 'Sandbox Microfinance Bank',
    account_number: p.deposit.accountNumber,
    account_name: `SATSBRIDGE / ${p.deposit.accountNumber.slice(4)}`,
    status: d.depositStatus,
    received_amount: d.receivedAtMs !== null ? p.source_amount : null,
    expires_at: iso(p.deposit.expiresAtMs),
    received_at: d.receivedAtMs !== null ? iso(d.receivedAtMs) : null,
    created_at: iso(p.deposit.createdAtMs),
  };
}

function details(p: MockPayment, nowMs: number) {
  const d = derive(p, nowMs);
  const settled = d.status !== 'PAYMENT_DETECTED';
  const provider = p.recipient_rail === 'KE_MOBILE_MONEY' ? 'tando-mock' : p.recipient_rail === 'MOBILE_MONEY' ? 'mobile-money-mock' : 'ngn-mock';
  return {
    ...publicIntent(p, nowMs, d),
    deposit: depositView(p, d),
    ledger: 'simulated',
    stellar_transactions: d.stellar.map((step) => stellarTx(p, step, nowMs, settled)),
    payout: d.payout && {
      payment_intent_id: p.id,
      provider,
      rail: p.recipient_rail,
      destination_currency: p.destination_currency,
      destination_amount: p.expected_payout,
      destination_reference: p.recipient_masked,
      account_name: p.recipient_name,
      provider_reference: `mockref_${p.id.slice(-10)}`,
      status: d.payout.status,
      failure_reason: d.payout.failureReason,
      created_at: iso(p.createdAtMs),
      completed_at: d.payout.completedAtMs ? iso(d.payout.completedAtMs) : null,
      reversed_at: d.payout.reversedAtMs ? iso(d.payout.reversedAtMs) : null,
      needs_review: false,
      review_reason: null,
    },
    refund: d.refund && {
      payment_intent_id: p.id,
      provider: 'ngn-mock',
      currency: p.source_currency,
      amount: p.source_amount,
      destination_reference: `${DEMO_SENDER.bank_code}:${mask(DEMO_SENDER.account_number)}`,
      account_name: DEMO_SENDER.account_name,
      provider_reference: `mockngn_${p.id}-refund`,
      status: d.refund.status,
      failure_reason: d.refund.status === 'failed' ? 'The bank rejected the refund (mock)' : null,
      attempts: 1,
      created_at: iso(d.refund.atMs),
      updated_at: iso(d.refund.doneAtMs ?? d.refund.atMs),
      completed_at: d.refund.status === 'completed' && d.refund.doneAtMs ? iso(d.refund.doneAtMs) : null,
    },
    events: d.events,
  };
}

function bearer(token: string | undefined): MockUser | null {
  if (!token) return null;
  const user = Object.values(load().users).find((u) => u.token === token);
  return user ?? null;
}

function currencyList() {
  const rails: Record<Currency, readonly Record<string, unknown>[]> = {
    XOF: [{ rail: 'MOBILE_MONEY', provider: 'mobile-money-mock', min_payout_minor: 500, max_payout_minor: 5_000_000, guaranteed_amount: true, countries: ['SN', 'CI', 'BJ', 'BF', 'ML', 'NE', 'TG', 'GW'] }],
    XAF: [{ rail: 'MOBILE_MONEY', provider: 'mobile-money-mock', min_payout_minor: 500, max_payout_minor: 5_000_000, guaranteed_amount: true, countries: ['CM', 'CF', 'TD', 'CG', 'GQ', 'GA'] }],
    GNF: [{ rail: 'MOBILE_MONEY', provider: 'mobile-money-mock', min_payout_minor: 5_000, max_payout_minor: 50_000_000, guaranteed_amount: true, countries: ['GN'] }],
    GHS: [{ rail: 'MOBILE_MONEY', provider: 'mobile-money-mock', min_payout_minor: 1_00, max_payout_minor: 50_000_00, guaranteed_amount: true, countries: ['GH'] }],
    KES: [{ rail: 'KE_MOBILE_MONEY', provider: 'tando-mock', min_payout_minor: 100_00, max_payout_minor: 150_000_00, guaranteed_amount: true }],
    NGN: [{ rail: 'NG_BANK_TRANSFER', provider: 'ngn-mock', min_payout_minor: 100_00, max_payout_minor: 5_000_000_00, guaranteed_amount: true }],
  };
  return (Object.keys(EXPONENT) as Currency[]).map((code) => ({
    code,
    name: NAMES[code],
    exponent: EXPONENT[code],
    stellar_asset: { code, issuer: MOCK_ISSUER },
    can_send: code === 'NGN',
    payout_rails: rails[code],
  }));
}

// Destination units per source unit through NGN -> USD -> destination, after the DEX spread.
function dexRate(src: Currency, dst: Currency): number {
  const hop = (10_000 - SPREAD_BPS) / 10_000;
  return (PER_USD[dst] / PER_USD[src]) * hop * hop;
}

function computeQuote(
  input: { source_currency?: Currency; destination_currency: Currency; amount: number; amount_side: 'source' | 'destination'; rail?: Rail },
  nowMs: number,
  userId: string,
): MockQuote | Response {
  const src = input.source_currency ?? 'NGN';
  const { destination_currency: dst, amount, amount_side: side } = input;
  if (src !== 'NGN') return mockError(400, 'validation_error', `senders can't pay in ${src} yet`);
  if (src === dst) return mockError(400, 'validation_error', 'source and destination currencies must differ');
  const dstInfo = currencyList().find((c) => c.code === dst);
  const rail = dstInfo?.payout_rails.find((r) => r.rail === (input.rail ?? dstInfo.payout_rails[0]?.rail));
  if (!dstInfo || !rail) return mockError(400, 'validation_error', 'Unsupported payout method.');
  if (!Number.isSafeInteger(amount) || amount <= 0) return mockError(400, 'validation_error', 'Amount must be positive.');

  const rate = dexRate(src, dst);
  const perMinor = rate * 10 ** (EXPONENT[dst] - EXPONENT[src]);
  const keep = (10_000 - SLIPPAGE_BPS) / 10_000;
  let sourceAmount: number;
  let fee: number;
  let send: number;
  let destAmount: number;
  if (side === 'source') {
    sourceAmount = amount;
    fee = Math.ceil((amount * FEE_BPS) / 10_000);
    send = amount - fee;
    destAmount = Math.floor(send * perMinor * keep);
  } else {
    destAmount = amount;
    send = Math.ceil(amount / keep / perMinor);
    sourceAmount = Math.ceil((send * 10_000) / (10_000 - FEE_BPS));
    fee = sourceAmount - send;
  }
  if (send <= 0 || destAmount <= 0) return mockError(400, 'validation_error', 'amount is too small to cover fees');
  const ttl = sourceAmount === 777 * 10 ** EXPONENT[src] ? QUOTE_TTL_SHORT_S : QUOTE_TTL_S;
  const expiresAtMs = nowMs + scaled(ttl);

  return {
    id: randomId('quote'),
    userId,
    source_currency: src,
    destination_currency: dst,
    destination_rail: rail.rail as Rail,
    payout_amount_guaranteed: true,
    amount_side: side,
    source_amount: sourceAmount,
    fee_source_amount: fee,
    fee_bps: FEE_BPS,
    send_amount: send,
    destination_amount: destAmount,
    exchange_rate: rate.toFixed(8).replace(/0+$/, ''),
    rate_source: 'simulated',
    conversion_path: [src, 'USD', dst],
    rate_as_of: iso(nowMs),
    expires_at: iso(expiresAtMs),
    created_at: iso(nowMs),
    expires_at_ms: expiresAtMs,
    delivered_stroops: Math.floor(send * perMinor * 10 ** (7 - EXPONENT[dst])),
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
    fee_source_amount: q.fee_source_amount,
    fee_bps: q.fee_bps,
    send_amount: q.send_amount,
    destination_amount: q.destination_amount,
    exchange_rate: q.exchange_rate,
    rate_source: q.rate_source,
    conversion_path: q.conversion_path,
    rate_as_of: q.rate_as_of,
    expires_at: q.expires_at,
    created_at: q.created_at,
  };
}

function recipientKey(r: MockRecipient): string {
  if (r.rail === 'NG_BANK_TRANSFER') return r.account_number ?? '';
  return r.phone_number ?? '';
}

function maskedRecipient(r: MockRecipient): string {
  if (r.rail === 'NG_BANK_TRANSFER') return `${r.bank_code}:${mask(r.account_number ?? '')}`;
  if (r.rail === 'KE_MOBILE_MONEY') return mask(r.phone_number ?? '');
  return `${(r.country ?? '').toUpperCase()}:${r.provider_code ?? ''}:${mask(r.phone_number ?? '')}`;
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
  if (parts[0] === 'dev' && parts[1] === 'payments' && parts.length === 4) return { name: `dev-${parts[3]}`, id: parts[2] };
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

  if (route.name === 'health') return json({ status: 'ok', version: 'mock', ledger: 'simulated' });
  if (route.name === 'currencies') return json(currencyList());
  if (route.name === 'institutions') {
    const currency = url.searchParams.get('currency');
    if (url.searchParams.get('rail') === 'MOBILE_MONEY' && !url.searchParams.get('country')) {
      return mockError(422, 'invalid_recipient', 'invalid recipient: country is required for mobile money operators');
    }
    if (currency === 'NGN') return json(BANKS.map((b) => ({ ...b })));
    if (currency === 'KES') return json([]);
    return json(MOBILE_OPERATORS.map((o) => ({ ...o })));
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
    return json(publicQuote(quote), 201);
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
    if (accountNumber.endsWith('9999')) return mockError(422, 'recipient_not_found', 'Account not found.');
    const id = randomId('res');
    const resolution: MockResolution = {
      id,
      userId: user.id,
      bank_code: bankCode,
      account_number: accountNumber,
      account_name: ACCOUNT_NAMES[Number(accountNumber[9]) % ACCOUNT_NAMES.length],
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
    if (recipient.rail === 'MOBILE_MONEY') {
      if (!/^\d{8,12}$/.test(recipient.phone_number ?? '') || !recipient.country || !recipient.provider_code) {
        return mockError(422, 'invalid_recipient', 'Mobile money details are not valid.');
      }
      recipientName = recipient.account_name?.trim() || null;
    }
    if (recipient.rail === 'KE_MOBILE_MONEY' && !/^0[17]\d{8}$/.test(recipient.phone_number ?? '')) {
      return mockError(422, 'invalid_recipient', 'Enter a Kenyan mobile number like 0712345678.');
    }
    const id = randomId('pay');
    const payment: MockPayment = {
      id,
      userId: user.id,
      quote_id: quote.id,
      source_currency: quote.source_currency,
      destination_currency: quote.destination_currency,
      source_amount: quote.source_amount,
      fee_source_amount: quote.fee_source_amount,
      send_amount: quote.send_amount,
      expected_payout: quote.destination_amount,
      exchange_rate: quote.exchange_rate,
      conversion_path: quote.conversion_path,
      delivered_stroops: quote.delivered_stroops,
      recipient_rail: recipient.rail,
      recipient_masked: maskedRecipient(recipient),
      recipient_name: recipientName,
      recipientKey: recipientKey(recipient),
      createdAtMs: nowMs,
      expiresAtMs: quote.expires_at_ms,
      deposit: null,
      devOutcome: null,
      refundOutcome: null,
    };
    state.payments[id] = payment;
    save();
    return json(publicIntent(payment, nowMs, derive(payment, nowMs)), 201);
  }

  const payment = route.id ? state.payments[route.id] : undefined;
  if (route.name.startsWith('payment-') || route.name.startsWith('dev-')) {
    if (!payment || payment.userId !== user.id) return mockError(404, 'not_found', 'Payment not found.');
  }
  if (!payment) {
    if (route.name === 'transactions') {
      const limit = Math.min(200, Math.max(1, Number(url.searchParams.get('limit') ?? '50') || 50));
      const list = Object.values(state.payments)
        .filter((p) => p.userId === user.id)
        .sort((a, b) => b.createdAtMs - a.createdAtMs)
        .slice(0, limit)
        .map((p) => publicIntent(p, nowMs, derive(p, nowMs)));
      return json(list);
    }
    return mockError(404, 'not_found', 'Route not found.');
  }

  const d = derive(payment, nowMs);

  if (route.name === 'payment-get') return json(details(payment, nowMs));

  if (route.name === 'payment-pay') {
    if (!payment.deposit) {
      if (nowMs >= payment.expiresAtMs) return mockError(410, 'quote_expired', 'quote has expired');
      const stall = payoutOutcome(payment) === 'stall';
      payment.deposit = {
        createdAtMs: nowMs,
        expiresAtMs: nowMs + scaled(stall ? STALL_DEPOSIT_TTL_S : DEPOSIT_TTL_S),
        accountNumber: `9${String(parseInt(randomHex(4), 16) % 1_000_000_000).padStart(9, '0')}`,
        receivedAtMs: null,
      };
      save();
    } else if (d.status !== 'AWAITING_PAYMENT') {
      return mockError(409, 'conflict', `payment is ${d.status}; no deposit instructions can be issued`);
    }
    return json(depositView(payment, derive(payment, nowMs)));
  }

  if (route.name === 'payment-refund') {
    if (d.status !== 'REFUND_PENDING') return mockError(409, 'conflict', `payment is ${d.status}; nothing to refund`);
    if (d.refund && d.refund.status !== 'failed') return mockError(409, 'conflict', 'a refund is already in progress');
    const resolution = state.resolutions[String(payload.resolution_id ?? '')];
    if (!resolution || resolution.userId !== user.id || resolution.account_number !== payload.account_number) {
      return mockError(400, 'validation_error', 'unknown resolution_id');
    }
    // The mock always knows the source account, so this path only follows a failed refund.
    payment.refundOutcome = { kind: 'success', atMs: nowMs + scaled(REFUND_DONE_S) };
    save();
    const after = details(payment, nowMs);
    return json({ ...after.refund, destination_reference: `${resolution.bank_code}:${mask(resolution.account_number)}`, account_name: resolution.account_name, status: 'submitted', completed_at: null });
  }

  if (route.name === 'dev-deposit') {
    if (d.status !== 'AWAITING_PAYMENT' || !payment.deposit) return mockError(409, 'conflict', 'call /pay first to get deposit instructions');
    payment.deposit.receivedAtMs = nowMs;
    save();
    return json({ outcome: 'processed' });
  }

  if (route.name === 'dev-payout') {
    const status = String(payload.status ?? '');
    if (status !== 'success' && status !== 'failed' && status !== 'reversed') {
      return mockError(400, 'validation_error', 'status must be success, failed or reversed.');
    }
    if (d.status !== 'PAYOUT_PENDING') return mockError(409, 'conflict', 'Payout is not pending.');
    payment.devOutcome = { kind: status, atMs: nowMs };
    save();
    return json({ outcome: 'processed' });
  }

  if (route.name === 'dev-refund') {
    const status = String(payload.status ?? '');
    if (status !== 'success' && status !== 'failed') return mockError(400, 'validation_error', 'status must be success or failed.');
    if (d.refund?.status !== 'submitted') return mockError(409, 'conflict', 'Refund is not in flight.');
    payment.refundOutcome = { kind: status, atMs: nowMs };
    save();
    return json({ outcome: 'processed' });
  }

  return mockError(404, 'not_found', 'Route not found.');
}

export function isMockApi(): boolean {
  return process.env.NEXT_PUBLIC_MOCK_API === 'true';
}
