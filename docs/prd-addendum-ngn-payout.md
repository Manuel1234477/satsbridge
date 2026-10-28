# PRD Addendum A — Nigerian Naira (NGN) Payout

**Applies to:** SatsBridge PRD v1.0
**Status:** Proposed
**Scope:** Add NGN as a destination (payout) currency. NGN as a source currency is out of scope for this addendum.

## A.1 Summary

Recipients in Nigeria receive Naira directly into a Nigerian bank account (and, where the provider supports it, a mobile-money / fintech wallet such as OPay, PalmPay or Moniepoint). Settlement still happens over Bitcoin/Lightning. After settlement, an NGN payout adapter sends the Naira out over Nigeria's instant interbank rail (NIBSS Instant Payment, "NIP").

Tando's coverage is centred on Kenya / M-Pesa, so NGN needs its own adapter behind the provider abstraction in FR-09 / FR-12. This is the first real test of that abstraction.

## A.2 New corridors

| Source | Destination | Notes |
|---|---|---|
| XOF | NGN | West Africa (UEMOA) → Nigeria |
| XAF | NGN | Central Africa (CEMAC) → Nigeria |
| GNF | NGN | Guinea → Nigeria |
| KES | NGN | Kenya → Nigeria |

For the demo, pick **one** of these (XOF → NGN is the most natural regional story) and treat the others as config.

## A.3 Functional requirements

| ID | Requirement | Description |
|---|---|---|
| FR-NGN-01 | NGN destination | `GET /api/v1/currencies` lists NGN with rail `NG_BANK_TRANSFER` (and `NG_WALLET` if supported). |
| FR-NGN-02 | Bank list | System returns the supported Nigerian banks and wallets, each with its bank code. |
| FR-NGN-03 | Account resolution | Before the user confirms, the system resolves the 10-digit NUBAN account number plus bank code to the account holder's name. The sender must see the name and confirm it. |
| FR-NGN-04 | Recipient validation | Reject a NUBAN that is not exactly 10 digits, an unknown bank code, or an account that fails resolution. Do not create a payment intent for an unresolved account. |
| FR-NGN-05 | Kobo precision | NGN amounts are stored as integer kobo (1 NGN = 100 kobo). Rounding happens once, on the server, at quote time. |
| FR-NGN-06 | NGN quote | The quote engine computes the source → BTC → NGN rate, applies fees, and records which NGN rate source was used. |
| FR-NGN-07 | Payout execution | After `SETTLED`, the NGN adapter starts exactly one transfer, using the payment intent ID as the provider's idempotency reference. |
| FR-NGN-08 | Reversal handling | If the provider reverses a transfer after reporting success, the system leaves `COMPLETED` and enters `FAILED` → `REFUND_PENDING`. |
| FR-NGN-09 | Limits | Enforce configurable per-transaction and daily NGN payout limits, so the demo stays within provider and bank KYC-tier limits. |
| FR-NGN-10 | Receipt | The receipt shows the bank name, the masked account number (`******1234`), the resolved account name, the NGN amount and the provider's session/transfer reference. |

## A.4 API additions

| Endpoint | Purpose |
|---|---|
| `GET /api/v1/payout-institutions?currency=NGN` | List banks and wallets with their codes. |
| `POST /api/v1/recipients/resolve` | `{ currency, bank_code, account_number }` → `{ account_name, resolution_id }` |

`POST /api/v1/payments` takes a `recipient` object for NGN payouts:

```json
{
  "quote_id": "q_123",
  "recipient": {
    "currency": "NGN",
    "rail": "NG_BANK_TRANSFER",
    "bank_code": "058",
    "account_number": "0123456789",
    "resolution_id": "res_456"
  }
}
```

The server re-checks that `resolution_id` belongs to the same bank code and account number. It never trusts an account name sent by the client.

`POST /api/v1/webhooks/provider` must handle the NGN provider's transfer events: success, failed and **reversed**. It verifies the provider's signature and deduplicates on the provider event ID.

## A.5 Data model changes

- **Payout** adds `rail`, `bank_code`, `account_number_masked`, `account_name`, `provider_reference`, `provider_session_id` and `reversed_at`.
- **RecipientResolution** (new): `id`, `currency`, `bank_code`, `account_number_hash`, `account_name`, `provider`, `created_at`, `expires_at`.
- Store the full account number only where the provider call needs it. Log it masked.

## A.6 State machine

No new top-level states. Two rules are added:

- `PAYOUT_PENDING` → `COMPLETED` only on a confirmed success event from the provider, or a confirmed status poll.
- `COMPLETED` → `FAILED` → `REFUND_PENDING` is allowed **only** for a provider reversal event. It is recorded as an AuditEvent.

## A.7 Provider options for the NGN adapter

| Option | Fit |
|---|---|
| Lightning-native NGN off-ramp (e.g. Bitnob, Yellow Card) | Takes sats and pays out NGN, so SatsBridge doesn't need to hold Naira liquidity. Best match for the PRD's settlement model. |
| Fiat payout API (e.g. Paystack Transfers, Flutterwave Payouts) | Strong bank coverage, account resolution and sandboxes. Needs a pre-funded NGN balance, so SatsBridge has to manage BTC → NGN conversion and liquidity. |
| Mock NGN adapter | Required for the demo fallback (PRD §22). It simulates resolve, success, failure, timeout and reversal. |

The adapter interface stays the same as every other rail: `resolve_recipient`, `quote_capacity`, `create_payout(idempotency_key, …)`, `get_payout_status` and `parse_webhook`.

## A.8 Risks specific to NGN

| Risk | Mitigation |
|---|---|
| FX rate divergence (official vs. market NGN rates) | Say which rate source is used. Keep quotes short-lived. Record the executed rate. |
| Wrong-account transfers | Account-name resolution and explicit sender confirmation are mandatory. |
| Post-success reversals | Reversal webhook handling and state rule (A.6). |
| Bank/NIP downtime | Status polling fallback, timeouts, and a retry with the same idempotency reference. |
| Regulatory (CBN / SEC Nigeria rules on virtual-asset providers) | Sandbox only for the demo. Partner with a licensed provider for production. Get legal review before going live (PRD §14). |

## A.9 Acceptance criteria

- NGN appears as a destination currency, and an XOF → NGN quote (or the chosen demo corridor) shows the rate, fees, NGN amount and expiry.
- An invalid NUBAN or bank code is rejected before a payment intent is created.
- The resolved account name is shown to and confirmed by the sender.
- A settled Lightning payment triggers exactly one NGN payout, even if webhooks are replayed.
- A reversal event moves the payment out of `COMPLETED` and is visible to the user.
- The mock NGN adapter can demo the whole flow without live credentials.
