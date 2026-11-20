# StellarBridge backend

Rust API for cross-border payments between African currencies, settled on the [Stellar](https://stellar.org) network: [axum](https://github.com/tokio-rs/axum) 0.8, tokio, sqlx with SQLite (migrations run on startup), reqwest with rustls, and [`stellar-xdr`](https://crates.io/crates/stellar-xdr) + `ed25519-dalek` to build and sign Stellar transactions.

For what is built and what has been verified against live services, see [`../docs/implementation-status.md`](../docs/implementation-status.md).

## How a payment flows

```
quote (Stellar DEX) → payment → pay-in details → deposit received (bank transfer or mobile money)
  → Stellar settle: mint NGN + path payment NGN → USD → KES (one atomic tx, destMin = quoted payout)
  → local payout (M-Pesa, mobile money, bank) → completed → Stellar redeem (burn the KES)
                                              ↘ failed → Stellar reverse + NGN refund to the sender
```

Every supported currency has a token on Stellar (`NGN`, `KES`, `GHS`, `XOF`, `XAF`, `GNF`) issued by our **issuer** account, one token per unit of fiat held in our local accounts. Tokens only exist while a payment is in flight, in the **treasury** account:

1. The sender's money arrives on the **collection** rail of their currency: a bank transfer to a per-payment virtual account (NGN), or a mobile money / M-Pesa payment with a per-payment reference (KES, GHS, XOF, XAF, GNF). The examples below use NGN → KES; any pair works the same way.
2. One Stellar transaction, signed by the treasury and the issuer: the issuer mints the deposit as `NGN` to the treasury, and the treasury converts `NGN` minus the fee into the destination token with a **strict-send path payment** through the `USD` bridge token on the Stellar DEX. Its `destMin` is the quoted payout, so the recipient gets at least what they were quoted, or nothing happens and the sender is refunded.
3. The payout rail pays the recipient locally. When it confirms, the destination tokens are **redeemed** (burned to the issuer).
4. If the payout fails, the conversion is **reversed** on the DEX (and the fee tokens burned), and the full deposit goes back to the sender's bank account.

A **market maker** account (set up by `stellar_setup`, not used by the API) keeps two-sided offers between every currency and `USD` on the DEX, priced from live FX rates plus a spread. That is what the path payments trade against.

## Run

Requires Rust 1.88 or newer.

```bash
cp .env.example .env
cargo run --bin stellar_setup   # creates the testnet accounts, trustlines and DEX liquidity;
                                # prints STELLAR_*_SECRET lines: paste them into .env
cargo run                       # http://localhost:8080/api/v1/health
cargo test                      # 55 tests; no network or credentials needed
```

No network? Set `STELLAR_LEDGER=simulated`: an in-memory ledger priced from the FX feed, with the same outcome rules as the real one. Nothing reaches Stellar, and quotes say `rate_source: "simulated"`.

Docker: `docker build -t stellarbridge-api . && docker run -p 8080:8080 --env-file .env -v stellarbridge-data:/data stellarbridge-api`

The crate builds two binaries: `stellarbridge` (the API, what `cargo run` starts) and `stellar_setup` ([below](#stellar-setup)).

### Live checks

An opt-in test runs against Stellar testnet and skips when the keys aren't set:

```bash
# Read-only: issuer/treasury accounts and trustlines, then NGN -> every currency path on the DEX.
cargo test --test stellar_live -- --ignored --nocapture
```

## Stellar

### Stellar setup

`stellar_setup` is idempotent; run it once, and again whenever you want to re-price the offers or after a testnet reset:

1. Creates the issuer, treasury and market maker accounts with friendbot (keys from `.env`, or generated and printed).
2. Adds trustlines from the treasury and market maker to every token and `USD`.
3. Tops the market maker's inventory up to twice `MM_DEPTH_USD` per currency (minted by the issuer).
4. Places or re-prices a sell offer each way between every currency and `USD`, at the reference rate (`FX_PROVIDERS`) plus `MM_SPREAD_BPS`.

At startup the API checks that the issuer and treasury exist and that the treasury trusts every token, and refuses to start otherwise.

### Exactly-once settlement

Each Stellar step (`settle`, `redeem`, `reverse`) is signed, **stored**, then submitted, so a crash or timeout never loses a transaction that may have been applied:

- An unknown outcome (e.g. Horizon's 504) is resolved later by looking up the hash. Resubmitting the same envelope is harmless: its sequence number can only be used once.
- A transaction counts as never applied only when the network says so, or when it is still missing 30 s after its `maxTime` (`STELLAR_TX_TIMEOUT_SECS`). Only then is a new one built.
- At most one live transaction per payment and kind (a partial unique index).
- Building and submitting treasury transactions is serialized in-process, since they share one sequence number. That caps throughput at roughly one settlement per ledger close (~5 s); see [Not done yet](#not-done-yet).

A settlement that fails on the ledger (`tx_failed`, e.g. `op_under_dest_min` because the DEX moved past the slippage buffer) ends the payment in `REFUND_PENDING`. Transaction-level rejections (`tx_bad_seq`, `tx_too_late`) are retried with a fresh transaction.

### Quotes

`POST /quotes` asks Horizon for the best path (`/paths/strict-send`, or `/paths/strict-receive` when `amount_side` is `destination`):

- `fee_source_amount` = `FEE_BPS` of the amount plus `FLAT_FEE_MINOR`, in the source currency.
- `send_amount` = what is converted on Stellar.
- `destination_amount` = the DEX's output for `send_amount`, minus `SLIPPAGE_BPS`. This is the path payment's `destMin`, so it is guaranteed.
- `exchange_rate` = destination units per source unit converted, at the DEX price; `conversion_path` = e.g. `["NGN","USD","KES"]`.

## Collections (money in)

Senders can pay in any supported currency. `POST /payments/{id}/pay` returns the instructions for it, with a `method`:

| Currency | `method` | Instructions |
|---|---|---|
| NGN | `bank_transfer` | Per-payment account number, bank, account name |
| KES | `mobile_money` | M-Pesa Paybill number and a per-payment account reference |
| GHS, XOF, XAF, GNF | `mobile_money` | Mobile money merchant number, networks, and a per-payment reference |

The money must arrive within `DEPOSIT_EXPIRY_SECS`, or the payment expires. The provider reports it on `POST /webhooks/collections` (signed like `/webhooks/provider`), with the account it came from in that currency's payout format, which is where refunds go:

```json
{ "event_id": "...", "reference": "<the deposit reference>", "amount_minor": 1000000, "currency": "KES",
  "sender": { "rail": "KE_MOBILE_MONEY", "phone_number": "0722000111" } }
```

- **Underpaid** deposits are refunded in full without touching Stellar.
- **Late** deposits (after the payment expired) are refunded.
- **Overpaid** deposits settle the quoted amount; the extra is flagged in the audit trail.
- A **second** deposit for the same payment is logged for manual return.

No collection provider is wired yet, so no currency is sendable until one is registered in `build_collections`. Deposits are reported on `POST /webhooks/collections`.

## Payouts (money out)

Rails are behind one trait (`PayoutAdapter`) and also carry refunds to senders in their own currency. No provider is wired yet; register them in `build_payouts`. Planned rails:

| Currency | Rail | Provider |
|---|---|---|
| KES | `KE_MOBILE_MONEY` (M-Pesa) | none yet |
| GHS, XOF, XAF, GNF | `MOBILE_MONEY` | none yet |
| NGN | `NG_BANK_TRANSFER` | none yet |

## Refunds

A payment that can't be completed moves to `REFUND_PENDING`: underpaid or late deposit, failed settlement, failed payout, or a payout the provider reverses after success. The **full deposit** goes back in the sender's currency, over that currency's payout rail (NGN bank transfer, M-Pesa, or mobile money):

- **Automatically** to the account the transfer came from, when the collection provider reported it.
- Otherwise the sender (or an admin) calls `POST /payments/{id}/refund` with an account in the source currency, shaped like a payment recipient: `{"rail":"KE_MOBILE_MONEY","phone_number":...}`, a `MOBILE_MONEY` wallet, or an NGN bank account with a `resolution_id` from `/recipients/resolve`.

One refund per payment: a failed refund can be retried to another account, but one in flight or done can't be started again. Refund status arrives on the same provider webhooks as payouts.

## Recovery worker

Every `RECOVERY_POLL_SECS` (default 10; `0` disables it):

1. Unpaid payments past their deposit deadline expire.
2. Stellar transactions with no recorded outcome are looked up, resubmitted, or rebuilt once provably expired; and steps that never started (a settle after a crash, a redeem after a payout, a reverse after a failure, an automatic refund) are run.
3. Unsubmitted payouts are resubmitted, after asking the provider whether an earlier attempt went through. Rails that can't answer go to manual review. Exponential backoff from `PAYOUT_RETRY_BACKOFF_SECS`, up to `PAYOUT_MAX_ATTEMPTS`.
4. Accepted payouts and refunds with no webhook after `PAYOUT_UNCONFIRMED_AFTER_SECS` are checked with the provider.

## Configuration

Every variable is documented in [`.env.example`](.env.example). The main ones:

| Variable | Purpose | Default |
|---|---|---|
| `STELLAR_LEDGER` | `testnet` or `simulated` | `testnet` |
| `STELLAR_ISSUER_SECRET`, `STELLAR_TREASURY_SECRET` | Account keys from `stellar_setup` | **required** on testnet |
| `STELLAR_HORIZON_URL` | Horizon | testnet |
| `STELLAR_BRIDGE_ASSET` | Token paths route through | `USD` |
| `STELLAR_TX_TIMEOUT_SECS` | Transaction validity window | 60 |
| `FEE_BPS`, `FLAT_FEE_MINOR` | Fee, in the source currency (the flat fee is in its minor units, so keep it 0 when several currencies are sent) | 100 (1%), 0 |
| `SLIPPAGE_BPS` | Buffer between the DEX estimate and the guaranteed payout | 50 |
| `QUOTE_TTL_SECS`, `DEPOSIT_EXPIRY_SECS` | Quote lock, time to make the transfer | 120, 1800 |
| `PROVIDER_WEBHOOK_SECRET` | HMAC secret for `/webhooks/provider` and `/webhooks/collections` | **required** |
| `ADMIN_API_KEY` | Bearer token for `/admin/*`; disabled when empty | — |
| `CORS_ALLOWED_ORIGINS` | Browser origins allowed to call the API | none |
| `FX_PROVIDERS` | Reference rates for `stellar_setup` and the simulated ledger | `coinbase,coingecko,static` |

## Conventions

- **Amounts** are integers in minor units. `GET /currencies` gives each currency's `exponent`: XOF/XAF/GNF = 0, KES/NGN/GHS = 2. Stellar amounts (`delivered`, `operation.*`) are in stroops (7 decimals).
- **Errors** are `{"error": {"code", "message"}}`. Codes: `validation_error` 400, `unauthorized` 401, `invalid_signature` 401, `not_found` 404, `conflict` 409, `quote_expired` 410, `invalid_recipient` and `recipient_not_found` 422, `rate_limited` 429, `provider_unavailable` 503, `provider_rejected` 502, `internal_error` 500.
- **Exactly one payout per payment:** webhook events are deduplicated, a status changes only if it is still in the expected state, one payout row per payment, and submission attempts are claimed atomically.
- **Webhook authentication:** `/webhooks/provider` and `/webhooks/collections` use `X-StellarBridge-Signature: <hex HMAC-SHA256 of raw body>`.
- **Personal data:** full recipient and sender account details are never returned. Account numbers are masked.

## Authentication

`POST /users` creates an anonymous user and returns a bearer token **once**; only its SHA-256 is stored. Send it as `Authorization: Bearer <token>`. Users only see their own quotes and payments (others return 404). Admin routes use `ADMIN_API_KEY` instead.

## API (`/api/v1`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/users` | none | → `{user_id, token, created_at}`. Rate limited per IP |
| GET | `/health` | none | DB check, ledger, network, treasury account |
| GET | `/currencies` | none | Exponent, Stellar asset, `can_send`, payout rails with limits |
| GET | `/payout-institutions?currency=&rail=&country=` | none | Banks or operators. `country` is needed for mobile money |
| POST | `/recipients/resolve` | user | `{currency: "NGN", bank_code, account_number}` → `{resolution_id, account_name}` |
| POST | `/quotes` | user | `{source_currency, destination_currency, amount, amount_side?: source\|destination, rail?}` |
| GET | `/quotes/{id}` | user | |
| POST | `/payments` | user | `{quote_id, recipient}` |
| POST | `/payments/{id}/pay` | user | Pay-in instructions (bank transfer or mobile money); safe to repeat |
| GET | `/payments/{id}` | user | Status, deposit, Stellar transactions with explorer links, payout, refund, audit trail |
| POST | `/payments/{id}/refund` | user | Empty body: to the source account. Or a recipient in the source currency |
| GET | `/transactions?limit=50` | user | The caller's recent payments |
| POST | `/webhooks/collections` | HMAC | Deposit reported by the collection provider |
| POST | `/webhooks/provider` | HMAC | `{event_id, provider_reference, status: success\|failed\|reversed, reason?}` for payouts and refunds |
| GET | `/admin/summary` | admin | Counts, corridors, completion rate, timings, ledger, treasury, rails |
| GET | `/admin/payments?status=&needs_review=&limit=` | admin | |
| GET | `/admin/payments/{id}` | admin | Same as the receipt |
| POST | `/admin/payments/{id}/retry-payout` | admin | `{force?}` |
| POST | `/admin/payments/{id}/fail-payout` | admin | `{reason}` → reverse on Stellar and refund |
| POST | `/admin/payments/{id}/refund` | admin | Same body as the user route |
| POST | `/admin/recovery/run` | admin | One recovery pass |

Recipients:

```json
{ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }
{ "rail": "MOBILE_MONEY", "country": "GH", "provider_code": "MTN", "phone_number": "0241234567", "account_name": "Kwame Mensah" }
{ "rail": "NG_BANK_TRANSFER", "bank_code": "058", "account_number": "0123456789", "resolution_id": "res_..." }
```


## Layout

```
src/
  api/          handlers and routing: user, admin, webhooks; auth and rate limits
  services/     quotes (DEX pricing + fees), payments (orchestration, `advance`), ledger (exactly-once
                Stellar transactions), refunds, recovery (worker), recipients, users, admin
  adapters/
    stellar/    trait, keys, transaction building/signing (XDR), Horizon client + ledger, simulated ledger
    collection/ trait + registry
    payout/     trait + registry
    fx/         coinbase, coingecko, static; cache + fallback chain (USD rates)
  domain/       Currency, money units, PaymentStatus state machine, Recipient and Rail
  db.rs         models and queries; `transition()` is the only way a status changes
  bin/stellar_setup.rs    testnet accounts, trustlines and DEX liquidity
migrations/     SQLite schema
tests/          end-to-end flows through the router on the simulated ledger, and an opt-in
                live test against Stellar testnet
```

## Not done yet

- **Real collection providers.** There are no pay-in rails yet. Real ones (Paystack/Flutterwave/Monnify for NGN, M-Pesa Daraja/C2B for KES, mobile money aggregators for GHS and the CFA francs, or Stellar anchors via SEP-24) plug in behind `CollectionAdapter`.
- **Real anchors / reserves.** Our issuer's tokens are test assets backed by nothing. In production each token would be issued by a licensed anchor (or backed 1:1 by our local accounts), and SEP-31 could replace the payout leg.
- **Throughput.** One treasury account means one transaction per ledger. Channel accounts would allow parallel submission.
- **Market making.** Offers are re-priced only when `stellar_setup` runs. A live market maker (or a liquidity pool) should follow the FX feed.
- **Mainnet.** Testnet only; the config refuses any other network passphrase.
- **Real payout providers.** There are no payout rails yet; real providers (or SEP-31 anchors) plug in behind `PayoutAdapter`.
