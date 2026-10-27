'use client';

import { apiRequest, getStoredToken, setStoredToken, clearStoredToken } from './client';
import {
  UserSchema,
  QuoteSchema,
  PaymentIntentSchema,
  PaymentDetailsSchema,
  CurrencyInfoSchema,
  PayoutInstitutionSchema,
  RecipientResolutionSchema,
  TransactionListSchema,
  type User,
  type Quote,
  type PaymentIntent,
  type PaymentDetails,
  type CurrencyInfo,
  type PayoutInstitution,
  type RecipientResolution,
  type Recipient,
  type TransactionList,
} from './schemas';

// User endpoints
export async function createUser(contact?: string): Promise<User> {
  return apiRequest('/users', {
    method: 'POST',
    requiresAuth: false,
    body: contact ? { contact } : {},
    schema: UserSchema,
  });
}

// Currency endpoints
export async function getCurrencies(): Promise<CurrencyInfo[]> {
  const response = await apiRequest('/currencies', {
    method: 'GET',
    requiresAuth: false,
    schema: CurrencyInfoSchema.array(),
  });
  return response;
}

export async function getPayoutInstitutions(currency: string, rail?: string): Promise<PayoutInstitution[]> {
  const params = new URLSearchParams({ currency });
  if (rail) params.append('rail', rail);
  const response = await apiRequest(`/payout-institutions?${params}`, {
    method: 'GET',
    requiresAuth: false,
    schema: PayoutInstitutionSchema.array(),
  });
  return response;
}

// Quote endpoints
export async function createQuote(input: {
  source_currency: string;
  destination_currency: string;
  amount: number;
  amount_side?: 'SOURCE' | 'DESTINATION';
  rail?: string;
}): Promise<Quote> {
  return apiRequest('/quotes', {
    method: 'POST',
    requiresAuth: true,
    body: input,
    schema: QuoteSchema,
  });
}

export async function getQuote(id: string): Promise<Quote> {
  return apiRequest(`/quotes/${id}`, {
    method: 'GET',
    requiresAuth: true,
    schema: QuoteSchema,
  });
}

// Recipient endpoints
export async function resolveRecipient(input: {
  currency: string;
  bank_code: string;
  account_number: string;
}): Promise<RecipientResolution> {
  return apiRequest('/recipients/resolve', {
    method: 'POST',
    requiresAuth: true,
    body: input,
    schema: RecipientResolutionSchema,
  });
}

// Payment endpoints
export async function createPayment(input: {
  quote_id: string;
  recipient: Recipient;
}): Promise<PaymentIntent> {
  return apiRequest('/payments', {
    method: 'POST',
    requiresAuth: true,
    body: input,
    schema: PaymentIntentSchema,
  });
}

export async function payPayment(id: string): Promise<PaymentDetails> {
  return apiRequest(`/payments/${id}/pay`, {
    method: 'POST',
    requiresAuth: true,
    body: {},
    schema: PaymentDetailsSchema,
  });
}

export async function getPayment(id: string): Promise<PaymentDetails> {
  return apiRequest(`/payments/${id}`, {
    method: 'GET',
    requiresAuth: true,
    schema: PaymentDetailsSchema,
  });
}

export async function refundPayment(id: string, paymentRequest: string): Promise<void> {
  await apiRequest(`/payments/${id}/refund`, {
    method: 'POST',
    requiresAuth: true,
    body: { payment_request: paymentRequest },
    schema: PaymentDetailsSchema,
  });
}

export async function getTransactions(limit = 50): Promise<TransactionList> {
  const response = await apiRequest(`/transactions?limit=${limit}`, {
    method: 'GET',
    requiresAuth: true,
    schema: TransactionListSchema,
  });
  return response;
}

// Dev endpoints
export async function simulatePayout(id: string, status: string): Promise<PaymentDetails> {
  return apiRequest(`/dev/payments/${id}/payout`, {
    method: 'POST',
    requiresAuth: true,
    body: { status },
    schema: PaymentDetailsSchema,
  });
}

// Auth helpers
export { getStoredToken, setStoredToken, clearStoredToken };

// Ensure user is authenticated, creating a new user if needed
export async function ensureUser(): Promise<void> {
  const token = getStoredToken();
  if (!token) {
    try {
      const user = await createUser();
      setStoredToken(user.token);
    } catch (error) {
      console.error('Failed to create user:', error);
      throw error;
    }
  }
}
