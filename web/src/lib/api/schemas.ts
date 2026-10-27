import { z } from 'zod';

// Enums
export const PaymentStatusSchema = z.enum([
  'CREATED',
  'QUOTED',
  'AWAITING_PAYMENT',
  'PAYMENT_DETECTED',
  'SETTLED',
  'PAYOUT_PENDING',
  'COMPLETED',
  'EXPIRED',
  'FAILED',
  'REFUND_PENDING',
  'REFUNDED',
]);
export type PaymentStatus = z.infer<typeof PaymentStatusSchema>;

export const CurrencySchema = z.enum(['XOF', 'XAF', 'GNF', 'KES', 'NGN']);
export type Currency = z.infer<typeof CurrencySchema>;

export const RailSchema = z.enum([
  'KE_MOBILE_MONEY',
  'NG_BANK_TRANSFER',
  'LIGHTNING_ADDRESS',
  'MOBILE_MONEY',
]);
export type Rail = z.infer<typeof RailSchema>;

export const LightningStatusSchema = z.enum(['PENDING', 'SETTLED']);
export type LightningStatus = z.infer<typeof LightningStatusSchema>;

export const PayoutStatusSchema = z.enum(['PENDING', 'SUBMITTED', 'COMPLETED', 'FAILED', 'REVERSED']);
export type PayoutStatus = z.infer<typeof PayoutStatusSchema>;

export const AmountSideSchema = z.enum(['SOURCE', 'DESTINATION']);
export type AmountSide = z.infer<typeof AmountSideSchema>;

// Models
export const QuoteSchema = z.object({
  id: z.string(),
  source_currency: CurrencySchema,
  destination_currency: CurrencySchema,
  destination_rail: RailSchema,
  payout_amount_guaranteed: z.boolean(),
  amount_side: AmountSideSchema,
  source_amount: z.number().int(),
  destination_amount: z.number().int(),
  amount_sats: z.number().int(),
  fee_sats: z.number().int(),
  fee_source_amount: z.number().int(),
  fee_bps: z.number().int(),
  exchange_rate: z.string(),
  rate_source: z.string(),
  rate_as_of: z.string().datetime(),
  expires_at: z.string().datetime(),
  created_at: z.string().datetime(),
});
export type Quote = z.infer<typeof QuoteSchema>;

export const RecipientSchema = z.discriminatedUnion('rail', [
  z.object({
    rail: z.literal('KE_MOBILE_MONEY'),
    phone_number: z.string(),
  }),
  z.object({
    rail: z.literal('NG_BANK_TRANSFER'),
    bank_code: z.string(),
    account_number: z.string(),
    resolution_id: z.string().optional(),
  }),
  z.object({
    rail: z.literal('LIGHTNING_ADDRESS'),
    address: z.string(),
  }),
  z.object({
    rail: z.literal('MOBILE_MONEY'),
    country: z.string(),
    provider_code: z.string(),
    phone_number: z.string(),
    account_name: z.string().optional(),
  }),
]);
export type Recipient = z.infer<typeof RecipientSchema>;

export const PaymentIntentSchema = z.object({
  id: z.string(),
  quote_id: z.string(),
  source_currency: CurrencySchema,
  destination_currency: CurrencySchema,
  source_amount: z.number().int(),
  expected_payout: z.number().int(),
  amount_sats: z.number().int(),
  fee_sats: z.number().int(),
  exchange_rate: z.string(),
  recipient_rail: RailSchema,
  recipient_masked: z.string(),
  recipient_name: z.string().nullable(),
  status: PaymentStatusSchema,
  failure_reason: z.string().nullable(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  expires_at: z.string().datetime(),
  refund_payment_hash: z.string().nullable(),
  refund_sats: z.number().int().nullable(),
  refunded_at: z.string().datetime().nullable(),
});
export type PaymentIntent = z.infer<typeof PaymentIntentSchema>;

export const LightningPaymentSchema = z.object({
  payment_intent_id: z.string(),
  payment_request: z.string(),
  payment_hash: z.string(),
  amount_sats: z.number().int(),
  status: LightningStatusSchema,
  expires_at: z.string().datetime(),
  settled_at: z.string().datetime().nullable(),
  created_at: z.string().datetime(),
});
export type LightningPayment = z.infer<typeof LightningPaymentSchema>;

export const PayoutSchema = z.object({
  payment_intent_id: z.string(),
  provider: z.string(),
  rail: RailSchema,
  destination_currency: CurrencySchema,
  destination_amount: z.number().int(),
  destination_reference: z.string(),
  account_name: z.string().nullable(),
  provider_reference: z.string().nullable(),
  status: PayoutStatusSchema,
  failure_reason: z.string().nullable(),
  created_at: z.string().datetime(),
  completed_at: z.string().datetime().nullable(),
  reversed_at: z.string().datetime().nullable(),
  needs_review: z.boolean(),
  review_reason: z.string().nullable(),
});
export type Payout = z.infer<typeof PayoutSchema>;

export const AuditEventSchema = z.object({
  id: z.number().int(),
  payment_intent_id: z.string(),
  event_type: z.string(),
  from_status: PaymentStatusSchema.nullable(),
  to_status: PaymentStatusSchema.nullable(),
  detail: z.string().nullable(),
  created_at: z.string().datetime(),
});
export type AuditEvent = z.infer<typeof AuditEventSchema>;

export const CurrencyInfoSchema = z.object({
  code: CurrencySchema,
  name: z.string(),
  exponent: z.number().int(),
  can_send: z.boolean(),
  payout_rails: z.array(
    z.object({
      rail: RailSchema,
      provider: z.string(),
      min_payout_minor: z.number().int(),
      max_payout_minor: z.number().int(),
      guaranteed_amount: z.boolean(),
      countries: z.array(z.string()).optional(),
    })
  ),
});
export type CurrencyInfo = z.infer<typeof CurrencyInfoSchema>;

export const PayoutInstitutionSchema = z.object({
  code: z.string(),
  name: z.string(),
});
export type PayoutInstitution = z.infer<typeof PayoutInstitutionSchema>;

export const UserSchema = z.object({
  user_id: z.string(),
  token: z.string(),
  created_at: z.string().datetime(),
});
export type User = z.infer<typeof UserSchema>;

export const RecipientResolutionSchema = z.object({
  resolution_id: z.string(),
  account_name: z.string(),
});
export type RecipientResolution = z.infer<typeof RecipientResolutionSchema>;

export const PaymentDetailsSchema = PaymentIntentSchema.extend({
  lightning: LightningPaymentSchema.nullable(),
  lnurl: z.string().nullable(),
  payout: PayoutSchema.nullable(),
  events: z.array(AuditEventSchema),
});
export type PaymentDetails = z.infer<typeof PaymentDetailsSchema>;

export const ErrorResponseSchema = z.object({
  error: z.object({
    code: z.string(),
    message: z.string(),
  }),
});
export type ErrorResponse = z.infer<typeof ErrorResponseSchema>;

export const TransactionListSchema = z.object({
  transactions: z.array(PaymentDetailsSchema),
});
export type TransactionList = z.infer<typeof TransactionListSchema>;
