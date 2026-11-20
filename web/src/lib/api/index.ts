import { apiRequest } from './client';
import {
  CurrencyInfoSchema,
  DepositSchema,
  PaymentDetailsSchema,
  PaymentIntentSchema,
  PayoutInstitutionSchema,
  QuoteSchema,
  RecipientResolutionSchema,
  RefundSchema,
  type AmountSide,
  type CurrencyInfo,
  type Deposit,
  type PayoutInstitution,
  type PaymentDetails,
  type PaymentIntent,
  type Quote,
  type Recipient,
  type RecipientResolution,
  type Refund,
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

// Payment instructions (bank transfer or mobile money); created on the first call, the same afterwards.
export function payPayment(id: string): Promise<Deposit> {
  return apiRequest({
    method: 'POST',
    path: `/payments/${encodeURIComponent(id)}/pay`,
    body: {},
    auth: true,
    schema: DepositSchema,
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

// Refund to an account in the source currency, shaped like a payment recipient (an NGN bank account
// needs a resolution_id). The account the money came from is used automatically when the provider
// reports it.
export function refundPayment(id: string, destination: Recipient): Promise<Refund> {
  return apiRequest({
    method: 'POST',
    path: `/payments/${encodeURIComponent(id)}/refund`,
    body: destination,
    auth: true,
    schema: RefundSchema,
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

export type { Deposit, PaymentDetails, PaymentIntent, Quote, Refund, RecipientResolution, PayoutInstitution, CurrencyInfo };
