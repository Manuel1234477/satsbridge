import type {
  AmountSide,
  CurrencyCode,
  CurrencyInfo,
  Institution,
  PayResponse,
  PaymentDetails,
  PaymentIntent,
  Quote,
  Rail,
  Recipient,
  ResolveResponse,
} from './types';

const BASE = (import.meta.env.VITE_API_URL ?? 'http://localhost:8080/api/v1').replace(/\/$/, '');
const TOKEN_KEY = 'satsbridge.token';

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

function storedToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

function storeToken(token: string) {
  try {
    localStorage.setItem(TOKEN_KEY, token);
  } catch {
    // Private mode: the token lives for this page load only.
  }
}

let memoryToken: string | null = null;
let signup: Promise<string> | null = null;

/** Anonymous account: created on first use, kept in this browser. A cleared browser loses history. */
async function ensureToken(): Promise<string> {
  const existing = memoryToken ?? storedToken();
  if (existing) return (memoryToken = existing);
  signup ??= request<{ token: string }>('POST', '/users', { auth: false, body: {} })
    .then((u) => {
      memoryToken = u.token;
      storeToken(u.token);
      return u.token;
    })
    .finally(() => {
      signup = null;
    });
  return signup;
}

function forgetToken() {
  memoryToken = null;
  try {
    localStorage.removeItem(TOKEN_KEY);
  } catch {
    // ignore
  }
}

interface Options {
  auth?: boolean;
  body?: unknown;
  query?: Record<string, string | number | undefined>;
  signal?: AbortSignal;
}

async function request<T>(method: string, path: string, opts: Options = {}, retried = false): Promise<T> {
  const { auth = true, body, query, signal } = opts;
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(query ?? {})) if (v !== undefined && v !== '') params.set(k, String(v));
  const url = `${BASE}${path}${params.size ? `?${params}` : ''}`;
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['content-type'] = 'application/json';
  if (auth) headers.authorization = `Bearer ${await ensureToken()}`;

  let res: Response;
  try {
    res = await fetch(url, { method, headers, signal, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    throw new ApiError(0, 'network', 'Cannot reach the SatsBridge server. Is it running?');
  }
  if (res.status === 401 && auth && !retried) {
    // The server no longer knows this token (for example after a database reset): start a new account.
    forgetToken();
    return request<T>(method, path, opts, true);
  }
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) {
    const e = data?.error;
    throw new ApiError(res.status, e?.code ?? 'error', e?.message ?? `Request failed (${res.status})`);
  }
  return data as T;
}

export const api = {
  currencies: () => request<CurrencyInfo[]>('GET', '/currencies', { auth: false }),
  institutions: (currency: CurrencyCode, rail?: Rail, country?: string) =>
    request<Institution[]>('GET', '/payout-institutions', { auth: false, query: { currency, rail, country } }),
  resolve: (currency: CurrencyCode, bank_code: string, account_number: string) =>
    request<ResolveResponse>('POST', '/recipients/resolve', { body: { currency, bank_code, account_number } }),
  createQuote: (
    body: {
      source_currency: CurrencyCode;
      destination_currency: CurrencyCode;
      amount: number;
      amount_side: AmountSide;
      rail?: Rail;
    },
    signal?: AbortSignal,
  ) => request<Quote>('POST', '/quotes', { body, signal }),
  createPayment: (quote_id: string, recipient: Recipient) =>
    request<PaymentIntent>('POST', '/payments', { body: { quote_id, recipient } }),
  payment: (id: string, signal?: AbortSignal) => request<PaymentDetails>('GET', `/payments/${id}`, { signal }),
  pay: (id: string) => request<PayResponse>('POST', `/payments/${id}/pay`),
  refund: (id: string, payment_request: string) =>
    request<PaymentIntent>('POST', `/payments/${id}/refund`, { body: { payment_request } }),
  transactions: (limit = 50) => request<PaymentIntent[]>('GET', '/transactions', { query: { limit } }),
  /** Dev only: simulates the callback of a mock payout provider. */
  simulatePayout: (id: string, status: 'success' | 'failed') =>
    request<unknown>('POST', `/dev/payments/${id}/payout`, { body: { status } }),
};
