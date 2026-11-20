import { z } from 'zod';

// Wire formats verified against backend/src/db.rs, backend/src/domain/*.rs and
// backend/src/services/*.rs. Payment statuses and rails are SCREAMING_SNAKE_CASE;
// amount_side, deposit/stellar/payout/refund statuses and dev payout status are snake_case.
// Amounts are integers in minor units, except Stellar amounts, which are in stroops (7 decimals).

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

export const CurrencySchema = z.enum(['XOF', 'XAF', 'GNF', 'KES', 'NGN', 'GHS']);
export type Currency = z.infer<typeof CurrencySchema>;

export const RailSchema = z.enum(['KE_MOBILE_MONEY', 'NG_BANK_TRANSFER', 'MOBILE_MONEY']);
export type Rail = z.infer<typeof RailSchema>;

export const AmountSideSchema = z.enum(['source', 'destination']);
export type AmountSide = z.infer<typeof AmountSideSchema>;

export const DepositStatusSchema = z.enum(['awaiting', 'received', 'expired']);
export type DepositStatus = z.infer<typeof DepositStatusSchema>;

export const StellarTxStatusSchema = z.enum(['submitted', 'succeeded', 'failed']);
export const StellarTxKindSchema = z.enum(['settle', 'redeem', 'reverse']);
export type StellarTxKind = z.infer<typeof StellarTxKindSchema>;

export const RefundStatusSchema = z.enum(['pending', 'submitted', 'completed', 'failed']);

export const PayoutStatusSchema = z.enum(['pending', 'submitted', 'completed', 'failed', 'reversed']);
export type PayoutStatus = z.infer<typeof PayoutStatusSchema>;

export const QuoteSchema = z.object({
  id: z.string(),
  source_currency: CurrencySchema,
  destination_currency: CurrencySchema,
  destination_rail: RailSchema,
  payout_amount_guaranteed: z.boolean(),
  amount_side: AmountSideSchema,
  source_amount: z.number().int(),
  fee_source_amount: z.number().int(),
  fee_bps: z.number().int(),
  send_amount: z.number().int(),
  destination_amount: z.number().int(),
  exchange_rate: z.string(),
  // stellar-testnet or simulated
  rate_source: z.string(),
  // Asset codes along the Stellar DEX path, e.g. ["NGN", "USD", "KES"].
  conversion_path: z.array(z.string()),
  rate_as_of: z.string(),
  expires_at: z.string(),
  created_at: z.string(),
});
export type Quote = z.infer<typeof QuoteSchema>;

export const RecipientSchema = z.discriminatedUnion('rail', [
  z.object({ rail: z.literal('KE_MOBILE_MONEY'), phone_number: z.string() }),
  z.object({
    rail: z.literal('NG_BANK_TRANSFER'),
    bank_code: z.string(),
    account_number: z.string(),
    resolution_id: z.string(),
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
  fee_source_amount: z.number().int(),
  send_amount: z.number().int(),
  expected_payout: z.number().int(),
  exchange_rate: z.string(),
  conversion_path: z.array(z.string()),
  recipient_rail: RailSchema,
  recipient_masked: z.string(),
  recipient_name: z.string().nullable(),
  status: PaymentStatusSchema,
  failure_reason: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  expires_at: z.string(),
});
export type PaymentIntent = z.infer<typeof PaymentIntentSchema>;

// Bank transfer instructions for the sender (POST /payments/{id}/pay) and their status.
export const DepositSchema = z.object({
  payment_intent_id: z.string(),
  provider: z.string(),
  currency: CurrencySchema,
  expected_amount: z.number().int(),
  reference: z.string(),
  bank_name: z.string(),
  account_number: z.string(),
  account_name: z.string(),
  status: DepositStatusSchema,
  received_amount: z.number().int().nullable(),
  expires_at: z.string(),
  received_at: z.string().nullable(),
  created_at: z.string(),
});
export type Deposit = z.infer<typeof DepositSchema>;

export const StellarTxSchema = z.object({
  kind: StellarTxKindSchema,
  tx_hash: z.string(),
  operation: z.record(z.string(), z.unknown()),
  status: StellarTxStatusSchema,
  delivered: z.number().int().nullable(),
  ledger: z.number().int().nullable(),
  error: z.string().nullable(),
  valid_until: z.string(),
  created_at: z.string(),
  confirmed_at: z.string().nullable(),
  explorer_url: z.string().nullable(),
});
export type StellarTx = z.infer<typeof StellarTxSchema>;

export const RefundSchema = z.object({
  payment_intent_id: z.string(),
  provider: z.string(),
  currency: CurrencySchema,
  amount: z.number().int(),
  destination_reference: z.string(),
  account_name: z.string().nullable(),
  provider_reference: z.string().nullable(),
  status: RefundStatusSchema,
  failure_reason: z.string().nullable(),
  attempts: z.number().int(),
  created_at: z.string(),
  updated_at: z.string(),
  completed_at: z.string().nullable(),
});
export type Refund = z.infer<typeof RefundSchema>;

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
  created_at: z.string(),
  completed_at: z.string().nullable(),
  reversed_at: z.string().nullable(),
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
  created_at: z.string(),
});
export type AuditEvent = z.infer<typeof AuditEventSchema>;

// GET /payments/{id}: PaymentIntent fields are flattened into the response.
export const PaymentDetailsSchema = PaymentIntentSchema.extend({
  deposit: DepositSchema.nullable(),
  // stellar-testnet or simulated
  ledger: z.string(),
  stellar_transactions: z.array(StellarTxSchema),
  payout: PayoutSchema.nullable(),
  refund: RefundSchema.nullable(),
  events: z.array(AuditEventSchema),
});
export type PaymentDetails = z.infer<typeof PaymentDetailsSchema>;

export const CurrencyInfoSchema = z.object({
  code: CurrencySchema,
  name: z.string(),
  exponent: z.number().int(),
  stellar_asset: z.object({ code: z.string(), issuer: z.string() }).nullable(),
  // Senders can pay in this currency (a collection rail exists).
  can_send: z.boolean(),
  payout_rails: z.array(
    z.object({
      rail: RailSchema,
      provider: z.string(),
      min_payout_minor: z.number().int().nullable(),
      max_payout_minor: z.number().int().nullable(),
      guaranteed_amount: z.boolean(),
      countries: z.array(z.string()).optional(),
    })
  ),
});
export type CurrencyInfo = z.infer<typeof CurrencyInfoSchema>;

export const PayoutInstitutionSchema = z.object({
  code: z.string(),
  name: z.string(),
  kind: z.enum(['bank', 'wallet', 'mobile_money']),
});
export type PayoutInstitution = z.infer<typeof PayoutInstitutionSchema>;

export const UserSchema = z.object({
  user_id: z.string(),
  token: z.string(),
  created_at: z.string(),
});
export type User = z.infer<typeof UserSchema>;

export const RecipientResolutionSchema = z.object({
  resolution_id: z.string(),
  account_name: z.string(),
  bank_code: z.string(),
  account_number_masked: z.string(),
  expires_at: z.string(),
});
export type RecipientResolution = z.infer<typeof RecipientResolutionSchema>;

export const DevPayoutOutcomeSchema = z.object({
  outcome: z.string(),
});

export const ErrorResponseSchema = z.object({
  error: z.object({
    code: z.string(),
    message: z.string(),
  }),
});
