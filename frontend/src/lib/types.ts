// Mirrors the SatsBridge API (backend/README.md). Amounts are integers in minor units.

export type CurrencyCode = 'XOF' | 'XAF' | 'GNF' | 'KES' | 'NGN';
export type Rail = 'KE_MOBILE_MONEY' | 'NG_BANK_TRANSFER' | 'LIGHTNING_ADDRESS' | 'MOBILE_MONEY';
export type AmountSide = 'source' | 'destination';

export type PaymentStatus =
  | 'CREATED'
  | 'QUOTED'
  | 'AWAITING_PAYMENT'
  | 'PAYMENT_DETECTED'
  | 'SETTLED'
  | 'PAYOUT_PENDING'
  | 'COMPLETED'
  | 'EXPIRED'
  | 'FAILED'
  | 'REFUND_PENDING'
  | 'REFUNDED';

export interface PayoutRail {
  rail: Rail;
  provider: string;
  min_payout_minor: number | null;
  max_payout_minor: number | null;
  guaranteed_amount: boolean;
  countries?: string[];
}

export interface CurrencyInfo {
  code: CurrencyCode;
  name: string;
  exponent: number;
  payout_rails: PayoutRail[];
}

export interface Institution {
  code: string;
  name: string;
  kind: 'bank' | 'wallet' | 'mobile_money';
}

export interface ResolveResponse {
  resolution_id: string;
  account_name: string;
  bank_code: string;
  account_number_masked: string;
  expires_at: string;
}

export interface Quote {
  id: string;
  source_currency: CurrencyCode;
  destination_currency: CurrencyCode;
  destination_rail: Rail;
  payout_amount_guaranteed: boolean;
  amount_side: AmountSide;
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
}

export type Recipient =
  | { rail: 'KE_MOBILE_MONEY'; phone_number: string }
  | { rail: 'NG_BANK_TRANSFER'; bank_code: string; account_number: string; resolution_id: string }
  | { rail: 'LIGHTNING_ADDRESS'; address: string }
  | {
      rail: 'MOBILE_MONEY';
      country: string;
      provider_code: string;
      phone_number: string;
      account_name?: string;
    };

export interface PaymentIntent {
  id: string;
  quote_id: string;
  source_currency: CurrencyCode;
  destination_currency: CurrencyCode;
  source_amount: number;
  expected_payout: number;
  amount_sats: number;
  fee_sats: number;
  exchange_rate: string;
  recipient_rail: Rail;
  recipient_masked: string;
  recipient_name: string | null;
  status: PaymentStatus;
  failure_reason: string | null;
  created_at: string;
  updated_at: string;
  expires_at: string;
  refund_payment_hash: string | null;
  refund_sats: number | null;
  refunded_at: string | null;
}

export interface LightningPayment {
  payment_intent_id: string;
  payment_request: string;
  payment_hash: string;
  amount_sats: number;
  status: 'pending' | 'settled';
  expires_at: string;
  settled_at: string | null;
}

export interface Payout {
  provider: string;
  rail: Rail;
  destination_currency: CurrencyCode;
  destination_amount: number;
  account_name: string | null;
  provider_reference: string | null;
  status: 'pending' | 'submitted' | 'completed' | 'failed' | 'reversed';
  failure_reason: string | null;
  completed_at: string | null;
}

export interface AuditEvent {
  id: number;
  event_type: string;
  from_status: PaymentStatus | null;
  to_status: PaymentStatus | null;
  detail: string | null;
  created_at: string;
}

export interface PaymentDetails extends PaymentIntent {
  lightning: LightningPayment | null;
  lnurl: string | null;
  payout: Payout | null;
  events: AuditEvent[];
}

export interface PayResponse extends LightningPayment {
  lnurl: string | null;
}
