import { apiRequest } from './client';
import {
  CurrencyInfoSchema,
  DevPayoutOutcomeSchema,
  PayResponseSchema,
  PaymentDetailsSchema,
  PaymentIntentSchema,
  PayoutInstitutionSchema,
  QuoteSchema,
  RecipientResolutionSchema,
  type AmountSide,
  type CurrencyInfo,
  type PayoutInstitution,
  type PayResponse,
  type PaymentDetails,
  type PaymentIntent,
  type Quote,
  type Recipient,
  type RecipientResolution,
  type Currency,
  type Rail,
} from './schemas';

export { ensureSession, clearStoredToken, getStoredToken } from './client';

export function getCurrencies(): Promise<CurrencyInfo[]> {
  return apiRequest({
    method: 'GET',
    path: '/currencies',
    auth: false,
    schema: CurrencyInfoSchema.array(),
  });
}

// Mobile money operators are per country: the backend rejects MOBILE_MONEY lookups without one.
export function getPayoutInstitutions(currency: Currency, rail?: Rail, country?: string): Promise<PayoutInstitution[]> {
  const params = new URLSearchParams({ currency });
  if (rail) params.set('rail', rail);
  if (country) params.set('country', country);
  return apiRequest({
    method: 'GET',
    path: `/payout-institutions?${params.toString()}`,
    auth: false,
    schema: PayoutInstitutionSchema.array(),
  });
}

export interface CreateQuoteInput {
  source_currency: Currency;
  destination_currency: Currency;
  amount: number;
  amount_side: AmountSide;
  rail?: Rail;
}

export function createQuote(input: CreateQuoteInput): Promise<Quote> {
  return apiRequest({ method: 'POST', path: '/quotes', body: input, auth: true, schema: QuoteSchema });
}

export function getQuote(id: string): Promise<Quote> {
  return apiRequest({ method: 'GET', path: `/quotes/${encodeURIComponent(id)}`, auth: true, schema: QuoteSchema });
}

export function resolveRecipient(input: {
  currency: 'NGN';
  bank_code: string;
  account_number: string;
}): Promise<RecipientResolution> {
  return apiRequest({
    method: 'POST',
    path: '/recipients/resolve',
    body: input,
    auth: true,
    schema: RecipientResolutionSchema,
  });
}

export function createPayment(input: { quote_id: string; recipient: Recipient }): Promise<PaymentIntent> {
  return apiRequest({
    method: 'POST',
    path: '/payments',
    body: input,
    auth: true,
    schema: PaymentIntentSchema,
  });
}

export function payPayment(id: string): Promise<PayResponse> {
  return apiRequest({
    method: 'POST',
    path: `/payments/${encodeURIComponent(id)}/pay`,
    body: {},
    auth: true,
    schema: PayResponseSchema,
  });
}

export function getPayment(id: string): Promise<PaymentDetails> {
  return apiRequest({
    method: 'GET',
    path: `/payments/${encodeURIComponent(id)}`,
    auth: true,
    schema: PaymentDetailsSchema,
  });
}

export function refundPayment(id: string, paymentRequest: string): Promise<PaymentIntent> {
  return apiRequest({
    method: 'POST',
    path: `/payments/${encodeURIComponent(id)}/refund`,
    body: { payment_request: paymentRequest },
    auth: true,
    schema: PaymentIntentSchema,
  });
}

export function getTransactions(limit = 50): Promise<PaymentIntent[]> {
  return apiRequest({
    method: 'GET',
    path: `/transactions?limit=${limit}`,
    auth: true,
    schema: PaymentIntentSchema.array(),
  });
}

export function simulatePayout(id: string, status: 'success' | 'failed' | 'reversed'): Promise<{ outcome: string }> {
  return apiRequest({
    method: 'POST',
    path: `/dev/payments/${encodeURIComponent(id)}/payout`,
    body: { status },
    auth: true,
    schema: DevPayoutOutcomeSchema,
  });
}

export type { PaymentDetails, PaymentIntent, PayResponse, Quote, RecipientResolution, PayoutInstitution, CurrencyInfo };
