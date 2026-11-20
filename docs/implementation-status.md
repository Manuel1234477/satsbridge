# SatsBridge — Implementation Status

**As of:** 2026-11-20
**Scope:** backend (`backend/`) and web app (`web/`) after the move from Bitcoin/Lightning to Stellar.
**Build:** compiles with no warnings, clippy clean. 63 backend tests pass (36 unit, 27 end-to-end), plus 2 opt-in live tests. The web app has 37 Vitest tests and 3 Playwright browser tests.
**Related docs:** [backend README](../backend/README.md) (setup, API, demo) · [web README](../web/README.md) · [demo script](demo-script.md)

---

## 1. Summary

A sender in Nigeria pays naira by bank transfer. The payment settles on **Stellar testnet** as one atomic transaction: the deposit is minted as an `NGN` token and path-paid into the destination token (`KES`, `GHS`, `XOF`, `XAF`, `GNF`) over the Stellar DEX. Its `destMin` is the quoted payout. A local rail then pays the recipient. Afterwards the destination tokens are burned, or, if the payout failed, the conversion is reversed on-chain and the naira refunded.

| Integration | Status | Used for |
|---|---|---|
| Stellar testnet (Horizon) | **Live, verified end to end**: accounts, trustlines, DEX offers, quotes from path finding, settle/redeem/reverse transactions, explorer links, through the API and the web UI | Settlement and FX |
| Coinbase / CoinGecko exchange rates | Live (USD rates) | Pricing the market maker's offers; the simulated ledger |
| NGN collection (virtual accounts) | **Mock only** | Receiving the sender's naira |
| Bitnob | Built and tested against a fake server and the local stand-in; **never run against the real API** | Mobile money and NGN bank payouts |
| Payout mocks (M-Pesa, mobile money, NGN bank) | Mock | Demos |

## 2. Architecture

```
web (Next.js) ──HTTP──> api (axum)
                          ├─ services: quotes · payments (advance) · ledger · refunds · recovery
                          ├─ adapters/stellar: HorizonLedger (testnet) | SimulatedLedger
                          ├─ adapters/collection: mock NGN virtual accounts
                          ├─ adapters/payout: Tando mock, mobile money mock, NGN mock, Bitnob
                          └─ SQLite: quotes, payments, deposits, stellar_transactions, payouts, refunds, audit
```

**Stellar accounts** (created by `stellar_setup`):

| Account | Role |
|---|---|
| Issuer | Issues `NGN`, `KES`, `GHS`, `XOF`, `XAF`, `GNF` and the `USD` bridge token. Mints deposits, receives burns |
| Treasury | Holds tokens while a payment is in flight. Source of every payment transaction |
| Market maker | Two-sided offers between each currency and `USD`, at the reference rate plus a spread. Not used by the API |

## 3. Payment lifecycle

```
CREATED → QUOTED → AWAITING_PAYMENT → PAYMENT_DETECTED → SETTLED → PAYOUT_PENDING → COMPLETED
                         │                   │                          │              │
                         ↓                   ↓                          ↓              ↓ (reversal)
                      EXPIRED ──(late)──> REFUND_PENDING <── FAILED <───┴──────────────┘
                                               ↓
                                            REFUNDED
```

| Step | What happens |
|---|---|
| `AWAITING_PAYMENT` | Bank transfer details issued (per-payment account number), deadline `DEPOSIT_EXPIRY_SECS` |
| `PAYMENT_DETECTED` | Deposit reported by the collection webhook; Stellar settle in progress |
| `SETTLED` | Settle transaction applied (mint + path payment); payout requested in the same DB transaction |
| `COMPLETED` | Payout confirmed; redeem (burn) transaction follows |
| `REFUND_PENDING` | Underpaid or late deposit, failed settlement, failed or reversed payout. Reverse transaction (if settled) and automatic refund to the source account |

Every status change is a compare-and-set with an audit event. `payments::advance` is the single idempotent driver; webhooks and the recovery worker both call it.

## 4. What was verified on Stellar testnet

Run on 2026-11-20 with fresh accounts from `stellar_setup`:

- `stellar_setup` created and funded the three accounts, added 14 trustlines, minted inventory, and placed 12 offers at live Coinbase rates. A rerun found everything in place and re-priced the offers in place.
- `stellar_live` test: NGN 100,000 quoted into every currency through `NGN → USD → X`.
- API, NGN → KES: ₦150,000 quoted at 0.0953 KES/NGN (KSh 14,087.32 guaranteed). Settle transaction applied: Horizon shows a 2-operation transaction (mint ₦150,000, path payment ₦148,500 → KSh 14,158.12 via USD). Mock payout success → redeem transaction applied. Status `COMPLETED`.
- API, NGN → GHS with a failed payout: settle, then reverse applied; refund of ₦50,000 to the source account; `REFUNDED`. Treasury afterwards: every token 0 except ₦1,500 (the completed payment's fee).
- Web UI against the testnet backend: quote with the DEX path, M-Pesa recipient, bank details, simulated transfer, "On Stellar" panel with explorer links, receipt. No console errors; every response passed the Zod schemas.

## 5. Exactly-once guarantees

| Risk | Guard |
|---|---|
| Duplicate webhook | `processed_webhooks` (source, event_id) |
| Two settlements for one payment | Partial unique index: one live transaction per payment and kind |
| Lost transaction after a crash or timeout | Signed envelope stored before submission; outcome looked up by hash; resubmission of the same envelope is harmless |
| Rebuilding a transaction that later applies | Only after the network rejects it, or it is still missing 30 s past its `maxTime` |
| Two payouts | One payout row per payment; attempts claimed atomically; provider lookup before any resubmission |
| Two refunds | One refund row per payment; a new one only after the previous one failed |
| Concurrent treasury transactions | Building and submitting serialized in-process (one sequence number) |

Covered by end-to-end tests on the simulated ledger: Horizon timeouts after applying, transactions lost before reaching the network (resubmitted, or rebuilt after expiry), the DEX moving below the quote (`UnderDestmin` → refund), underpaid, late and duplicate deposits, payout failure and reversal, refunds with and without a known source account.

## 6. Security

- Issuer and treasury secret keys are only read from the environment; `Config` and `Keypair` are deliberately not `Debug`.
- Webhooks: HMAC-SHA256 (`/webhooks/provider`, `/webhooks/collections`), HMAC-SHA512 (`/webhooks/bitnob`).
- Users: anonymous bearer tokens, only their SHA-256 stored; other users' payments return 404.
- Recipient and sender account details are stored in full only for execution, and returned masked.
- Rate limits per user, per IP and on sign-up.
- Testnet only: the config refuses any other network passphrase.

## 7. Configuration

See [`backend/.env.example`](../backend/.env.example). Required on testnet: `STELLAR_ISSUER_SECRET`, `STELLAR_TREASURY_SECRET` (from `stellar_setup`) and `PROVIDER_WEBHOOK_SECRET`. `STELLAR_LEDGER=simulated` needs neither Stellar keys nor network.

## 8. Tests

| Suite | Count | Covers |
|---|---|---|
| Unit (`cargo test --lib`) | 36 | Transaction signing (verifiable signatures, hash, memo), result decoding, amounts and price approximation, DEX quotes, FX parsing and caching, state machine, keys |
| `payment_flow` | 13 | NGN → KES end to end, failure and reversal with on-chain undo, refunds, DEX slippage, Stellar timeouts and lost transactions, expiry and late deposits, signatures, limits, Bitnob webhooks |
| `platform` | 11 | Auth and privacy, rate limits, payout recovery and review, admin, NGN → XAF/GNF/GHS |
| `bitnob_sandbox` | 3 | The Bitnob adapter against the local stand-in |
| `stellar_live`, `bitnob_live` | 2 (ignored) | Live, read-only checks |
| Web: Vitest | 37 | Mock API contract (every response through the Zod schemas), money, status, errors |
| Web: Playwright | 3 | Happy path, failure with reversal and refund, provider reversal (mock mode) |

## 9. Known gaps and next steps

1. **Real NGN collection.** A virtual-account provider (Paystack, Flutterwave, Monnify) or an NGN anchor (SEP-24) behind `CollectionAdapter`.
2. **Backed assets.** The tokens are test assets. Production needs licensed anchors (or reserves 1:1 with our local accounts) and SEP-31 for the receiving side.
3. **Throughput.** One treasury sequence number serializes settlements; channel accounts would lift this.
4. **Live market making.** Offers are re-priced only when `stellar_setup` runs. A daemon (or AMM pools) should follow the FX feed.
5. **Deadline for settlement.** If the DEX has no path for a long time, a received deposit waits in `PAYMENT_DETECTED` (retried by recovery) rather than being refunded.
6. **Overpayments** are flagged but the excess is not refunded automatically.
7. **Bitnob** has never been run against the real API; no real KES provider.
8. **Project name.** "SatsBridge" still refers to Bitcoin.
