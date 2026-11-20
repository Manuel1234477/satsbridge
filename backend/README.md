# SatsBridge backend

Rust API for cross-border payments from Nigerian naira to other African currencies, settled on the [Stellar](https://stellar.org) network: [axum](https://github.com/tokio-rs/axum) 0.8, tokio, sqlx with SQLite (migrations run on startup), reqwest with rustls, and [`stellar-xdr`](https://crates.io/crates/stellar-xdr) + `ed25519-dalek` to build and sign Stellar transactions.

For what is built and what has been verified against live services, see [`../docs/implementation-status.md`](../docs/implementation-status.md).

## How a payment flows

```
quote (Stellar DEX) → payment → bank transfer details → NGN deposit received
  → Stellar settle: mint NGN + path payment NGN → USD → KES (one atomic tx, destMin = quoted payout)
  → local payout (M-Pesa, mobile money, bank) → completed → Stellar redeem (burn the KES)
                                              ↘ failed → Stellar reverse + NGN refund to the sender
```

Every supported currency has a token on Stellar (`NGN`, `KES`, `GHS`, `XOF`, `XAF`, `GNF`) issued by our **issuer** account, one token per unit of fiat held in our local accounts. Tokens only exist while a payment is in flight, in the **treasury** account:

1. The sender's naira arrives by bank transfer on a per-payment virtual account (the **collection** rail).
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
cargo test                      # 63 tests; no network or credentials needed
```

No network? Set `STELLAR_LEDGER=simulated`: an in-memory ledger priced from the FX feed, with the same outcome rules as the real one. Nothing reaches Stellar, and quotes say `rate_source: "simulated"`.

Docker: `docker build -t satsbridge-api . && docker run -p 8080:8080 --env-file .env -v satsbridge-data:/data satsbridge-api`

The crate builds three binaries: `satsbridge` (the API, what `cargo run` starts), `stellar_setup` ([below](#stellar-setup)) and `bitnob_sandbox`, a local stand-in for Bitnob ([below](#bitnob-local-sandbox)).

### Live checks

Two opt-in tests run against real services and skip when their variables aren't set:

```bash
# Read-only: issuer/treasury accounts and trustlines, then NGN -> every currency path on the DEX.
cargo test --test stellar_live -- --ignored --nocapture
# Read-only: Bitnob credentials, limits, bank list and field rules per BITNOB_COUNTRIES.
cargo test --test bitnob_live -- --ignored --nocapture
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

- `fee_source_amount` = `FEE_BPS` of the amount plus `FLAT_FEE_MINOR`, in naira.
- `send_amount` = what is converted on Stellar.
- `destination_amount` = the DEX's output for `send_amount`, minus `SLIPPAGE_BPS`. This is the path payment's `destMin`, so it is guaranteed.
- `exchange_rate` = destination units per naira converted, at the DEX price; `conversion_path` = e.g. `["NGN","USD","KES"]`.

## Collections (money in)

`POST /payments/{id}/pay` returns bank transfer details: a per-payment account number, the bank, the account name and the exact amount. The deposit must arrive within `DEPOSIT_EXPIRY_SECS`, or the payment expires.

The provider reports deposits on `POST /webhooks/collections` (signed like `/webhooks/provider`):

```json
{ "event_id": "...", "reference": "<the deposit reference / account number>", "amount_minor": 15000000, "currency": "NGN",
  "sender": { "bank_code": "058", "account_number": "0123456789", "account_name": "ADA OBI" } }
```

- **Underpaid** deposits are refunded in full without touching Stellar.
- **Late** deposits (after the payment expired) are refunded.
- **Overpaid** deposits settle the quoted amount; the extra is flagged in the audit trail.
- A **second** deposit to the same account is logged for manual return.

Only a mock provider is built (`MOCK_COLLECTIONS=true`): a Paystack/Flutterwave-style dedicated virtual account per payment. With `DEV_ROUTES=true`, `POST /dev/payments/{id}/deposit` simulates the transfer (from a demo account, or `{"sender": null}` for an unknown one).

## Payouts (money out)

Rails are behind one trait; mocks fill in wherever no real provider is configured (`MOCK_PAYOUTS=true`):

| Currency | Rail | Provider |
|---|---|---|
| KES | `KE_MOBILE_MONEY` (M-Pesa) | Tando mock, or Bitnob `KE` |
| GHS, XOF, XAF, GNF | `MOBILE_MONEY` | mock, or Bitnob per country |
| NGN | `NG_BANK_TRANSFER` | mock, or Bitnob `NG`. Carries **refunds** to senders |

### Bitnob

Enabled by listing countries in `BITNOB_COUNTRIES`: `NG` is a Naira bank transfer, and `KE`, `GH`, `SN`, `CI`, `CM` and others are mobile money. Each country is checked against Bitnob at startup (limits and beneficiary field rules).

- **Payout:** quote → initialize → **finalize (irreversible)**, debited from your pre-funded Bitnob balance in `BITNOB_FROM_ASSET` (fund it with USDC from the treasury in production).
- **Status:** `POST /webhooks/bitnob`, HMAC-SHA512 in `x-bitnob-signature`. `BITNOB_CALLBACK_URL` (or `PUBLIC_BASE_URL`) must be publicly reachable.
- **Retries:** each attempt uses its own reference (`{payment_id}`, `{payment_id}-r2`, ...) so reconciliation finds every attempt.

#### Bitnob local sandbox

`bitnob_sandbox` answers the Bitnob endpoints SatsBridge uses, verifies the same request signatures, and sends signed webhooks. It proves our side of the integration, not Bitnob's behaviour.

```bash
cargo run --bin bitnob_sandbox            # http://127.0.0.1:9100
# .env for the API, with the same BITNOB_CLIENT_ID/SECRET/WEBHOOK_SECRET as the stand-in:
BITNOB_BASE_URL=http://127.0.0.1:9100
BITNOB_COUNTRIES=KE
PUBLIC_BASE_URL=http://127.0.0.1:8080
```

An account number ending in `9999` makes a stand-in payout expire (exercises the refund path). Settings: `SANDBOX_BIND`, `SANDBOX_AUTO_COMPLETE_SECS` (default 3), `SANDBOX_BALANCE_SATS`.

## Refunds

A payment that can't be completed moves to `REFUND_PENDING`: underpaid or late deposit, failed settlement, failed payout, or a payout the provider reverses after success. The **full deposit** goes back in naira over the NGN bank rail:

- **Automatically** to the account the transfer came from, when the collection provider reported it.
- Otherwise the sender (or an admin) calls `POST /payments/{id}/refund` with a verified NGN account (`bank_code`, `account_number`, `resolution_id` from `/recipients/resolve`).

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
| `FEE_BPS`, `FLAT_FEE_MINOR` | Fee, in naira | 100 (1%), 0 |
| `SLIPPAGE_BPS` | Buffer between the DEX estimate and the guaranteed payout | 50 |
| `QUOTE_TTL_SECS`, `DEPOSIT_EXPIRY_SECS` | Quote lock, time to make the transfer | 120, 1800 |
| `MOCK_COLLECTIONS`, `NGN_MAX_DEPOSIT_MINOR` | Mock NGN collection rail and its cap (kobo) | `true`, ₦5M |
| `PROVIDER_WEBHOOK_SECRET` | HMAC secret for `/webhooks/provider` and `/webhooks/collections` | **required** |
| `MOCK_PAYOUTS`, `NGN_MAX_PAYOUT_MINOR` | Mock payout rails; cap on the mock NGN rail | `true`, ₦500k |
| `BITNOB_*` | Bitnob corridors and credentials | off |
| `ADMIN_API_KEY` | Bearer token for `/admin/*`; disabled when empty | — |
| `CORS_ALLOWED_ORIGINS` | Browser origins allowed to call the API | none |
| `FX_PROVIDERS` | Reference rates for `stellar_setup` and the simulated ledger | `coinbase,coingecko,static` |
| `DEV_ROUTES` | Mounts the mock deposit/payout/refund simulators. **Never in production** | `false` |

## Conventions

- **Amounts** are integers in minor units. `GET /currencies` gives each currency's `exponent`: XOF/XAF/GNF = 0, KES/NGN/GHS = 2. Stellar amounts (`delivered`, `operation.*`) are in stroops (7 decimals).
- **Errors** are `{"error": {"code", "message"}}`. Codes: `validation_error` 400, `unauthorized` 401, `invalid_signature` 401, `not_found` 404, `conflict` 409, `quote_expired` 410, `invalid_recipient` and `recipient_not_found` 422, `rate_limited` 429, `provider_unavailable` 503, `provider_rejected` 502, `internal_error` 500.
- **Exactly one payout per payment:** webhook events are deduplicated, a status changes only if it is still in the expected state, one payout row per payment, and submission attempts are claimed atomically.
- **Webhook authentication:** `/webhooks/provider` and `/webhooks/collections` use `X-SatsBridge-Signature: <hex HMAC-SHA256 of raw body>`; `/webhooks/bitnob` uses Bitnob's HMAC-SHA512.
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
| POST | `/quotes` | user | `{source_currency?: "NGN", destination_currency, amount, amount_side?: source\|destination, rail?}` |
| GET | `/quotes/{id}` | user | |
| POST | `/payments` | user | `{quote_id, recipient}` |
| POST | `/payments/{id}/pay` | user | Bank transfer details (the deposit); safe to repeat |
| GET | `/payments/{id}` | user | Status, deposit, Stellar transactions with explorer links, payout, refund, audit trail |
| POST | `/payments/{id}/refund` | user | Empty body: to the source account. Or `{bank_code, account_number, resolution_id}` |
| GET | `/transactions?limit=50` | user | The caller's recent payments |
| POST | `/webhooks/collections` | HMAC | Deposit reported by the collection provider |
| POST | `/webhooks/provider` | HMAC | `{event_id, provider_reference, status: success\|failed\|reversed, reason?}` for payouts and refunds |
| POST | `/webhooks/bitnob` | HMAC-SHA512 | Bitnob payout callbacks |
| POST | `/dev/payments/{id}/deposit` | user | Dev only: `{amount_minor?, sender?}` simulates the bank transfer (mock collections) |
| POST | `/dev/payments/{id}/payout`, `/refund` | user | Dev only: `{status}` simulates a **mock** provider's callback |
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

## Demo: ₦150,000 → M-Pesa in Kenya

With `DEV_ROUTES=true`, after `stellar_setup`. `jq` reads the responses.

```bash
API=localhost:8080/api/v1; J='content-type: application/json'
TOKEN=$(curl -s -XPOST $API/users | jq -r .token); A="authorization: Bearer $TOKEN"
Q=$(curl -s -XPOST $API/quotes -H "$J" -H "$A" -d '{"source_currency":"NGN","destination_currency":"KES","amount":15000000}')
echo "$Q" | jq '{source_amount, fee_source_amount, destination_amount, exchange_rate, conversion_path}'
P=$(curl -s -XPOST $API/payments -H "$J" -H "$A" -d "{\"quote_id\":$(echo $Q | jq .id),\"recipient\":{\"rail\":\"KE_MOBILE_MONEY\",\"phone_number\":\"0712345678\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay -H "$A" | jq '{bank_name, account_number, expected_amount}'   # where to send the naira
curl -s -XPOST $API/dev/payments/$P/deposit -H "$A"                                               # the transfer arrives
curl -s $API/payments/$P -H "$A" | jq '{status, stellar: [.stellar_transactions[] | {kind, status, explorer_url}]}'
curl -s -XPOST $API/dev/payments/$P/payout -H "$J" -H "$A" -d '{"status":"success"}'              # mock M-Pesa confirms
curl -s $API/payments/$P -H "$A" | jq '{status, events: [.events[].event_type]}'                  # COMPLETED; KES burned
```

Use `{"status":"failed"}` instead to watch the conversion reverse on Stellar and the naira go back to the sender.

## Layout

```
src/
  api/          handlers and routing: user, admin, webhooks, dev simulators; auth and rate limits
  services/     quotes (DEX pricing + fees), payments (orchestration, `advance`), ledger (exactly-once
                Stellar transactions), refunds, recovery (worker), recipients, users, admin
  adapters/
    stellar/    trait, keys, transaction building/signing (XDR), Horizon client + ledger, simulated ledger
    collection/ trait + registry, mock NGN virtual accounts
    payout/     trait + registry, ngn_mock, tando_mock, mobile_money_mock
    bitnob/     signed client, payout adapter (bank + mobile money)
    fx/         coinbase, coingecko, static; cache + fallback chain (USD rates)
  domain/       Currency, money units, PaymentStatus state machine, Recipient and Rail
  db.rs         models and queries; `transition()` is the only way a status changes
  bin/stellar_setup.rs    testnet accounts, trustlines and DEX liquidity
  bin/bitnob_sandbox.rs   local stand-in for Bitnob
migrations/     SQLite schema
tests/          end-to-end flows through the router on the simulated ledger, the Bitnob sandbox,
                and opt-in live tests (Stellar testnet, Bitnob)
```

## Not done yet

- **Real collection provider.** NGN deposits come from a mock virtual-account provider. A real one (Paystack, Flutterwave, Monnify, or an NGN Stellar anchor via SEP-24) plugs in behind `CollectionAdapter`.
- **Real anchors / reserves.** Our issuer's tokens are test assets backed by nothing. In production each token would be issued by a licensed anchor (or backed 1:1 by our local accounts), and SEP-31 could replace the payout leg.
- **Throughput.** One treasury account means one transaction per ledger. Channel accounts would allow parallel submission.
- **Market making.** Offers are re-priced only when `stellar_setup` runs. A live market maker (or a liquidity pool) should follow the FX feed.
- **Mainnet.** Testnet only; the config refuses any other network passphrase.
- **Bitnob live verification** and a real KES provider, as before.
