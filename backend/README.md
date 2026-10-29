# SatsBridge backend

Rust API for Lightning-settled cross-border payments: [axum](https://github.com/tokio-rs/axum) 0.8, tokio, sqlx with SQLite (migrations run on startup), reqwest with rustls.

For what is built and what has been verified against live services, see [`../docs/implementation-status.md`](../docs/implementation-status.md).

## Run

Requires Rust 1.88 or newer.

```bash
cp .env.example .env # then set LNBITS_URL and LNBITS_INVOICE_KEY (required)
cargo run            # http://localhost:8080/api/v1/health
cargo test           # 71 tests (44 unit, 27 end-to-end); no network or credentials needed
```

Docker: `docker build -t satsbridge-api . && docker run -p 8080:8080 --env-file .env -v satsbridge-data:/data satsbridge-api`

Lightning is always real as far as SatsBridge is concerned: it needs an LNbits wallet (a real one, or the local [stand-in](#lnbits-local-sandbox)) and checks its keys at startup, refusing to start if they fail. Payout rails that no real provider serves are mocks (`MOCK_PAYOUTS=true`, the default). FX fetches live rates from Coinbase and CoinGecko.

The crate builds three binaries: `satsbridge` (the API, what `cargo run` starts), and two local stand-ins for outside services, `bitnob_sandbox` ([below](#bitnob-local-sandbox)) and `lnbits_sandbox` ([below](#lnbits-local-sandbox)).

### Live checks

Two opt-in tests run against real services. They are skipped when the variables aren't set:

```bash
# Read-only: Bitnob credentials (sandbox/production), limits, bank list and field rules for
# each country in BITNOB_COUNTRIES. Uses BITNOB_* from .env. Also works against the local sandbox.
cargo test --test bitnob_live -- --ignored --nocapture

# Receives and sends 21 sats on a real LNbits (needs two wallets on the same instance).
LNBITS_TEST_URL=... LNBITS_TEST_INVOICE_KEY=... LNBITS_TEST_ADMIN_KEY=... \
LNBITS_TEST_PAYEE_INVOICE_KEY=... cargo test --test lnbits_live -- --ignored --nocapture
```

## Providers

Every external system sits behind a trait. Lightning is LNbits only. For payouts, a mock fills in wherever no real provider serves a currency and rail.

| Trait | Mock | Real | Select with |
|---|---|---|---|
| `FxProvider` | `StaticFxProvider` | Bitnob, Coinbase and CoinGecko, cached, tried in order | `FX_PROVIDERS` |
| `LightningAdapter` | none | `LnbitsAdapter` (required) | `LNBITS_URL`, `LNBITS_INVOICE_KEY` |
| `PayoutAdapter` NGN `NG_BANK_TRANSFER` | `MockNgnAdapter` | `BitnobPayoutAdapter` | `BITNOB_COUNTRIES=NG` |
| `PayoutAdapter` `MOBILE_MONEY` (KE, XOF, XAF countries) | `MockMobileMoneyAdapter` (XOF, XAF) | `BitnobPayoutAdapter` | `BITNOB_COUNTRIES=KE,SN,CI,...` |
| `PayoutAdapter` NGN `LIGHTNING_ADDRESS` | none | `LightningAddressAdapter` (Tapnob) | `TAPNOB_DOMAINS` (empty disables it) |
| `PayoutAdapter` KES `KE_MOBILE_MONEY` | `MockTandoAdapter` | not built; use Bitnob `KE` instead | `MOCK_PAYOUTS` |

Payout adapters are registered by **currency and rail**. A quote can name a `rail`. Otherwise the currency's bank or mobile-money rail is used, real providers before mocks. A payment must use the same rail as its quote. `GET /currencies` shows what is available, with each rail's provider, limits and whether the payout amount is guaranteed.

### FX rates

`FX_PROVIDERS` is tried in order for each currency. The default is `bitnob,coinbase,coingecko` when `BITNOB_COUNTRIES` is set (so quotes use the rate Bitnob pays out at), otherwise `coinbase,coingecko`. Each live source is fetched at most every `FX_CACHE_TTL_SECS` (`FX_BITNOB_TTL_SECS` for Bitnob, which creates a throwaway payout quote per refresh). If a source is down, its last good prices are served for up to `FX_MAX_AGE_SECS`. Each quote records `rate_source` and `rate_as_of`.

| Source | XOF/XAF | GNF | KES | NGN |
|---|---|---|---|---|
| Bitnob payout quote | ✓ if a corridor is enabled | ✗ | ✓ if enabled | ✓ if enabled |
| Coinbase `/v2/exchange-rates` (no key) | via EUR peg | ✓ | ✓ | ✓ |
| CoinGecko `/simple/price` (optional key) | via EUR peg | ✗ | ✓ | ✓ |

Coinbase and CoinGecko always compute XOF and XAF as BTC/EUR × 655.957 (fixed peg). Those two are reference rates, not prices you can execute at. Bitnob's rate is the one actually paid out.

### Lightning (LNbits)

- **Startup check:** `GET /api/v1/wallet` with each key. SatsBridge refuses to start on a bad URL or key, or if `LNBITS_ADMIN_KEY` isn't actually an admin key. It logs the wallet name and balance.
- **Receiving:** invoices are created with `LNBITS_INVOICE_KEY`. If LNbits rejects `LNBITS_WEBHOOK_URL` (by default it only accepts domain names), the invoice is created without the webhook and the watcher settles it.
- **Settlement:** always confirmed by asking LNbits (`GET /api/v1/payments/{hash}`), in two ways:
  - The **settlement watcher** checks unpaid invoices every `LIGHTNING_POLL_SECS` (default 5). It also expires invoices that run out unpaid. It works without a public URL.
  - `POST /webhooks/lnbits` receives LNbits's invoice webhook (set `LNBITS_WEBHOOK_URL`). The webhook is unsigned, so it only triggers the same check.

  Both paths share an event id, so an invoice settles exactly once.
- **No fake settlement:** nothing settles a payment except LNbits reporting it paid. There is no shared-secret Lightning webhook and no dev "settle" route.
- **Sending** (Tapnob payouts and refunds): uses `LNBITS_ADMIN_KEY`. A payment counts as failed only when LNbits says it failed. A timeout or an in-flight payment is left unresolved, never retried blindly, and the recovery worker checks the outcome. Retrying an already-paid invoice returns the existing payment and moves no money (verified on a real LNbits).
- **LNURL-pay for senders:** with `PUBLIC_BASE_URL` set, each payment also has an `LNURL1...` code. A wallet that scans it gets the same single invoice as `POST /payments/{id}/pay`, so there is never a second payable invoice.
- **Invoice parsing:** invoices are decoded and their signatures verified. We skip the `lightning-invoice` crate's stricter checks, which reject invoices LNbits issues without a BOLT11 feature field.
- **Fees:** LNbits has no per-payment fee cap; the wallet's fee reserve applies. Refunds cap the routing fee at 0.5% (minimum 10 sats).

### Bitnob (bank transfer and mobile money)

Enabled by listing countries in `BITNOB_COUNTRIES`: `NG` is a Naira bank transfer (`NG_BANK_TRANSFER`), and `KE`, `SN`, `CI`, `NE`, `CM` and others are mobile money (`MOBILE_MONEY`). Each country is checked against Bitnob at startup (limits and beneficiary field rules), and the API refuses to start if any check fails.

- **Account check (NGN):** before the sender confirms, the account name is looked up (`GET /api/payouts/account-lookup`). Banks come from `GET /api/payouts/banks/NG`. These are 6-digit NIP codes, e.g. Access = `000014`; the mock uses 3-digit codes such as `058`. Mobile money has no lookup, so the recipient may need to supply `account_name`.
- **Payout:** after settlement, quote → initialize → **finalize (irreversible)**. The money is debited from your **pre-funded Bitnob balance** (`source: offchain`, asset `BITNOB_FROM_ASSET`). See [why funding is needed](../docs/implementation-status.md#7-why-a-funded-bitnob-account-is-needed).
- **Status:** arrives on `POST /webhooks/bitnob`, signed with HMAC-SHA512 in `x-bitnob-signature`. `payouts.withdrawal.success` → `COMPLETED`, `payouts.withdrawal.expired` → `FAILED`. `BITNOB_CALLBACK_URL` (or `PUBLIC_BASE_URL`) must be publicly reachable.
- **Amount:** the quoted destination amount is guaranteed. Any difference from Bitnob's rate is absorbed and logged.
- **Limits:** minimum and maximum payout come from Bitnob per country at startup. `NGN_MAX_PAYOUT_MINOR` caps only the mock rail.
- **Retries:** each attempt uses its own Bitnob `reference` (`{payment_id}`, then `{payment_id}-r2`, ...) so reconciliation can find every attempt. See [Recovery](#payout-recovery-and-refunds).

#### Bitnob local sandbox

Bitnob's real sandbox needs credentials from Bitnob. Until you have them, `bitnob_sandbox` is a local stand-in that answers the same endpoints SatsBridge uses (`whoami`, limits, banks and field rules per country, account lookup, quote, initialize, finalize, payout list and status). It verifies the same HMAC request signature, keeps a pretend balance that finalize debits, and sends signed `payouts.withdrawal.*` webhooks. It is built from our reading of Bitnob's spec, so it proves our side of the integration, not Bitnob's behaviour.

```bash
# Terminal 1: the stand-in (reads BITNOB_CLIENT_ID/SECRET and BITNOB_WEBHOOK_SECRET from .env)
cargo run --bin bitnob_sandbox            # http://127.0.0.1:9100

# .env for the API: use the same three BITNOB_* values as the stand-in, then
BITNOB_BASE_URL=http://127.0.0.1:9100
BITNOB_COUNTRIES=NG
PUBLIC_BASE_URL=http://127.0.0.1:8080     # the stand-in calls back to /api/v1/webhooks/bitnob
```

If `BITNOB_CLIENT_SECRET` or `BITNOB_WEBHOOK_SECRET` is empty the stand-in falls back to `sandbox-secret` and `sandbox-webhook-secret` and says so; put the same values in the API's `.env`. The API itself refuses to start when `BITNOB_COUNTRIES` is set and any of `BITNOB_CLIENT_ID`, `BITNOB_CLIENT_SECRET` or `BITNOB_WEBHOOK_SECRET` is empty.

| Setting | Default | Meaning |
|---|---|---|
| `SANDBOX_BIND` | `127.0.0.1:9100` | Listen address |
| `SANDBOX_AUTO_COMPLETE_SECS` | `3` | Seconds from finalize to the outcome webhook; `0` leaves payouts `processing` until you finish them by hand |
| `SANDBOX_BALANCE_SATS` | `10000000` | Starting balance |

Behaviour to use in demos:
- An account number ending in `9999` makes the payout **expire**, which exercises the failure and refund path. Any other number succeeds.
- Account number `0000000000` fails the NGN account lookup.
- Control endpoints (unauthenticated, not part of Bitnob's API): `GET /sandbox/state`, `POST /sandbox/payouts/{id}/succeed|expire`, `POST /sandbox/fund {"sats": n}`.
- Webhooks are retried until the API answers 2xx, with the same `event_id`.

The stand-in is covered by `tests/bitnob_sandbox.rs`, which starts it and drives the real Bitnob adapter through a success, an expiry and a rejected credential.

### LNbits local sandbox

Without a Lightning node, `lnbits_sandbox` is a local stand-in for the LNbits API. It implements what SatsBridge uses (`/api/v1/wallet` and `/api/v1/payments`) with LNbits's key rules: an invoice key creates and reads invoices, and only an admin key spends. Invoices are real signed regtest BOLT11 invoices, so SatsBridge's own decoding and checks run unchanged, and it sends the same double-encoded webhook LNbits does. No network or real money is involved.

SatsBridge still settles a payment only when this server reports the invoice paid. The sender's wallet is played on a separate page:

```bash
cargo run --bin lnbits_sandbox            # http://127.0.0.1:5000, prints the URL and keys to use
```

```
LNBITS_URL=http://127.0.0.1:5000
LNBITS_INVOICE_KEY=sandbox-invoice-key    # the stand-in's defaults; or set both in .env for both programs
LNBITS_ADMIN_KEY=sandbox-admin-key
```

Open `http://127.0.0.1:5000/sandbox` and press **Pay** next to an invoice SatsBridge created (or `POST /sandbox/pay/{payment_hash}`). The page also shows the pretend balance. Outgoing payments (refunds, Tapnob payouts) succeed at once, cost 1 sat, and are debited from `SANDBOX_LN_BALANCE_SATS` (default 1,000,000). `LNBITS_SANDBOX_BIND` changes the address. Covered by `tests/lnbits_sandbox.rs`.

Together with the Bitnob stand-in this runs the whole product locally, including payout webhooks and the refund path, with no external accounts.

### NGN: Tapnob Lightning Address (`LIGHTNING_ADDRESS`)

- **Recipient:** gives `name@tapnob.io`. Tapnob converts the sats and pays the recipient's linked bank account.
- **Payout:** we resolve the address over LNURL-pay, fetch an invoice for the net sats, check its amount and metadata hash, and pay it from LNbits. The payout completes when the Lightning payment succeeds.
- **Amount:** Tapnob converts at its own rate, so quotes carry `payout_amount_guaranteed: false`.
- **Safety:** only `TAPNOB_DOMAINS` (and their subdomains) are contacted, and redirects aren't followed.
- **Requirement:** needs `LNBITS_ADMIN_KEY` and an LNbits wallet with outbound liquidity.

### Payout recovery and refunds

A background **recovery worker** (every `RECOVERY_POLL_SECS`, default 30; `0` disables it) finishes what the normal path left unfinished:

1. **Unsubmitted payouts** (crash before submission, or an unconfirmed attempt). Before resubmitting, it asks the provider whether an earlier attempt already went through. Rails that can't answer that go to manual review instead. Retries back off exponentially from `PAYOUT_RETRY_BACKOFF_SECS` and stop after `PAYOUT_MAX_ATTEMPTS`, then the payout is flagged `needs_review`.
2. **Accepted payouts with no webhook** after `PAYOUT_UNCONFIRMED_AFTER_SECS`: it asks the provider for the status.
3. **Refunds whose Lightning outcome was unknown.**

A payout that fails (or a provider reversal after success) moves the payment to `REFUND_PENDING`. The sender then calls `POST /payments/{id}/refund` with a BOLT11 invoice for **exactly the sats they paid**, and SatsBridge pays it from LNbits (needs `LNBITS_ADMIN_KEY`). An admin can do the same. A refund is never paid twice: the refund's payment hash is locked before paying and kept while the outcome is unknown.

## Configuration

Every variable is documented in [`.env.example`](.env.example). The main ones:

| Variable | Purpose | Default |
|---|---|---|
| `DATABASE_URL` | SQLite path | `sqlite://satsbridge.db` |
| `PUBLIC_BASE_URL` | Public https origin of this server. Enables LNURL, and is the default base for the LNbits and Bitnob webhook URLs | — |
| `CORS_ALLOWED_ORIGINS` | Browser origins allowed to call the API (the frontend), comma-separated. `.env.example` sets `http://localhost:3000`; with none set, browsers are refused unless `DEV_ROUTES` is on | none |
| `TRUST_PROXY_HEADERS` | Use `X-Forwarded-For` for client IPs. Only behind a proxy you control | `false` |
| `ADMIN_API_KEY` | Bearer token for `/admin/*`; admin routes are disabled when empty | — |
| `RATE_LIMIT_PER_MINUTE`, `RATE_LIMIT_SIGNUPS_PER_HOUR` | Per user (per IP on public routes); new users per IP. `0` = off | 120, 20 |
| `PROVIDER_WEBHOOK_SECRET` | HMAC secret for `/webhooks/provider` | **required** |
| `QUOTE_TTL_SECS`, `INVOICE_EXPIRY_SECS` | Quote lock and invoice lifetime | 120, 600 |
| `FEE_BPS`, `FLAT_FEE_SATS` | SatsBridge fee | 100 (1%), 0 |
| `LNBITS_URL`, `LNBITS_INVOICE_KEY` | LNbits instance and receive key | **required** |
| `LNBITS_ADMIN_KEY` | Spend key, needed for Tapnob payouts and refunds | — |
| `LNBITS_WEBHOOK_URL` | Public URL of `/webhooks/lnbits` | `PUBLIC_BASE_URL/api/v1/webhooks/lnbits` |
| `LIGHTNING_POLL_SECS` | Settlement watcher interval; `0` is only allowed with a webhook URL | 5 |
| `BITNOB_COUNTRIES` | Bitnob corridors, e.g. `NG,KE,SN`; empty disables Bitnob | — |
| `BITNOB_CLIENT_ID`, `BITNOB_CLIENT_SECRET`, `BITNOB_WEBHOOK_SECRET` | Bitnob credentials (all **required** when countries are set). The secret selects sandbox or production, and startup logs which | — |
| `BITNOB_BASE_URL` | Override the Bitnob host, e.g. the local sandbox | `https://api.bitnob.com` |
| `BITNOB_CALLBACK_URL` | Where Bitnob posts payout events | `PUBLIC_BASE_URL/api/v1/webhooks/bitnob` |
| `BITNOB_FROM_ASSET` | Balance payouts are debited from: BTC, USDT or USDC | `BTC` |
| `MOCK_PAYOUTS` | Mock rails wherever no real provider serves a currency and rail. Set `false` in production | `true` |
| `NGN_MAX_PAYOUT_MINOR` | Cap on the mock NGN rail, in kobo | 50000000 (₦500k) |
| `TAPNOB_DOMAINS` | Lightning Address domains; empty disables the rail | `tapnob.io` |
| `RECOVERY_POLL_SECS`, `PAYOUT_MAX_ATTEMPTS`, `PAYOUT_RETRY_BACKOFF_SECS`, `PAYOUT_UNCONFIRMED_AFTER_SECS` | Recovery worker | 30, 5, 120, 600 |
| `FX_PROVIDERS` | `bitnob,coinbase,coingecko[,static]` | see [FX rates](#fx-rates) |
| `DEV_ROUTES` | Mounts the mock-payout callback simulator and allows any CORS origin when no allowlist is set. **Never in production** | `false` |

## Conventions

- **Amounts** are integers in minor units. `GET /currencies` gives each currency's `exponent`: XOF/XAF/GNF = 0, KES/NGN = 2. Bitcoin amounts are in sats.
- **Fees** are taken in sats. `fee_source_amount` shows them in the sender's currency.
- **Errors** are `{"error": {"code", "message"}}`. Codes: `validation_error` 400, `unauthorized` 401, `invalid_signature` 401, `not_found` 404, `conflict` 409, `quote_expired` 410, `invalid_recipient` and `recipient_not_found` 422, `rate_limited` 429, `provider_unavailable` 503, `provider_rejected` 502, `internal_error` 500.
- **Exactly one payout per payment:** repeated webhook events are discarded, status only changes if it's still in the expected state, the database allows one payout row per payment, and submission attempts are claimed atomically. The payment ID is the provider reference.
- **Webhook authentication:**
  - `/webhooks/provider`: `X-SatsBridge-Signature: <hex HMAC-SHA256 of raw body>`
  - `/webhooks/bitnob`: Bitnob's own HMAC-SHA512 signature
  - `/webhooks/lnbits`: confirmed with LNbits instead of a signature
- **Personal data:** full recipient details are never returned. Account numbers are masked.

## Authentication

Senders have no password. `POST /users` creates an anonymous user and returns a bearer token **once**; only its SHA-256 is stored. Send it as `Authorization: Bearer <token>` on every user route. A user can only see their own quotes, payments and transactions (other users' IDs return 404). A lost token cannot be recovered. Admin routes use `ADMIN_API_KEY` as the bearer token instead.

## API (`/api/v1`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/users` | none | `{contact?}` → `{user_id, token, created_at}`. Rate limited per IP |
| GET | `/health` | none | DB check, active Lightning node, whether LNURL is on |
| GET | `/currencies` | none | Currencies, exponents, payout rails with provider, limits and `guaranteed_amount` |
| GET | `/payout-institutions?currency=NGN[&rail=...][&country=SN]` | none | Banks or wallets with codes. `country` is needed for mobile money |
| POST | `/recipients/resolve` | user | `{currency: "NGN", bank_code, account_number}` → `{resolution_id, account_name}` |
| POST | `/quotes` | user | `{source_currency, destination_currency, amount, amount_side?: source\|destination, rail?}` |
| GET | `/quotes/{id}` | user | |
| POST | `/payments` | user | `{quote_id, recipient}` (see below) |
| GET | `/payments/{id}` | user | Status, invoice, `lnurl`, payout and audit trail (the receipt) |
| POST | `/payments/{id}/pay` | user | Returns the Lightning invoice and `lnurl`; safe to repeat |
| POST | `/payments/{id}/refund` | user | `{payment_request}`: BOLT11 for exactly the sats paid. Only in `REFUND_PENDING` |
| GET | `/transactions?limit=50` | user | The caller's most recent payments |
| GET | `/lnurlp/{id}`, `/lnurlp/{id}/callback?amount=<msat>` | none | LNURL-pay for wallets (LUD-06), any origin |
| POST | `/webhooks/provider` | HMAC | `{event_id, provider_reference, status: success\|failed\|reversed, reason?}` |
| POST | `/webhooks/bitnob` | HMAC-SHA512 | Bitnob payout callbacks |
| POST | `/webhooks/lnbits` | none | LNbits invoice webhook; always re-checked with LNbits |
| POST | `/dev/payments/{id}/payout` | user | Dev only: `{status}` simulates a callback from a **mock** payout provider. Refused for Bitnob and Tapnob |
| GET | `/admin/summary` | admin | Status counts, corridors, completion rate, average timings, rails |
| GET | `/admin/payments?status=&needs_review=&limit=` | admin | Payments with their payouts |
| GET | `/admin/payments/{id}` | admin | Same as the receipt |
| POST | `/admin/payments/{id}/retry-payout` | admin | `{force?}` clears the review flag and retries now |
| POST | `/admin/payments/{id}/fail-payout` | admin | `{reason}`: gives up on a payout the provider never accepted → `REFUND_PENDING` |
| POST | `/admin/payments/{id}/refund` | admin | `{payment_request}` |
| POST | `/admin/recovery/run` | admin | Runs one recovery pass and reports it |

Recipients:

```json
{ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }
{ "rail": "NG_BANK_TRANSFER", "bank_code": "000014", "account_number": "0123456789", "resolution_id": "res_..." }
{ "rail": "LIGHTNING_ADDRESS", "address": "ada@tapnob.io" }
{ "rail": "MOBILE_MONEY", "country": "SN", "provider_code": "OM", "phone_number": "771234567", "account_name": "Fatou Diallo" }
```

## Demos

Run with `DEV_ROUTES=true`. Each demo prints a real Lightning invoice. Pay it from any Lightning wallet, or from a second wallet on your LNbits instance. The settlement watcher picks up the payment within `LIGHTNING_POLL_SECS`. `jq` is used to read responses.

### XOF → NGN bank account (mock NGN rail)

Without `BITNOB_COUNTRIES`, the NGN bank rail is the mock, which uses 3-digit bank codes such as `058`. With Bitnob or its local sandbox, use the codes from `/payout-institutions?currency=NGN` (e.g. `000014`).

```bash
API=localhost:8080/api/v1; J='content-type: application/json'
TOKEN=$(curl -s -XPOST $API/users | jq -r .token); A="authorization: Bearer $TOKEN"
Q=$(curl -s -XPOST $API/quotes -H "$J" -H "$A" -d '{"source_currency":"XOF","destination_currency":"NGN","amount":100000}' | jq -r .id)
R=$(curl -s -XPOST $API/recipients/resolve -H "$J" -H "$A" -d '{"currency":"NGN","bank_code":"058","account_number":"0123456789"}' | jq -r .resolution_id)
P=$(curl -s -XPOST $API/payments -H "$J" -H "$A" -d "{\"quote_id\":\"$Q\",\"recipient\":{\"rail\":\"NG_BANK_TRANSFER\",\"bank_code\":\"058\",\"account_number\":\"0123456789\",\"resolution_id\":\"$R\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay -H "$A" | jq -r .payment_request   # pay this invoice
curl -s $API/payments/$P -H "$A" | jq -r .status                       # PAYOUT_PENDING once paid
curl -s -XPOST $API/dev/payments/$P/payout -H "$J" -H "$A" -d '{"status":"success"}'   # mock rails only
curl -s $API/payments/$P -H "$A" | jq '{status, expected_payout, payout, events: [.events[].event_type]}'
```

With the Bitnob local sandbox there is no need for the dev call: the stand-in sends the webhook itself a few seconds after the payout is submitted. Use an account number ending in `9999` to see the failure and refund path.

### XOF → NGN via a Tapnob Lightning Address

Needs `LNBITS_ADMIN_KEY` and sats in the LNbits wallet. This sends a real payment to the address.

```bash
Q=$(curl -s -XPOST $API/quotes -H "$J" -H "$A" -d '{"source_currency":"XOF","destination_currency":"NGN","amount":100000,"rail":"LIGHTNING_ADDRESS"}' | jq -r .id)
P=$(curl -s -XPOST $API/payments -H "$J" -H "$A" -d "{\"quote_id\":\"$Q\",\"recipient\":{\"rail\":\"LIGHTNING_ADDRESS\",\"address\":\"<name>@tapnob.io\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay -H "$A" | jq -r .payment_request   # pay this invoice
curl -s $API/payments/$P -H "$A" | jq '{status, payout}'                # COMPLETED once SatsBridge pays the address
```

## Layout

```
src/
  api/          handlers and routing: user, admin, webhooks, LNURL, dev simulation routes;
                bearer-token auth and rate limiting
  services/     quotes (FX + fee math), recipients (NGN resolution), payments (orchestration),
                settlement (invoice watcher), recovery (payout retry worker), refunds,
                lnurl (LNURL-pay for senders), users, admin (ops views and actions)
  adapters/
    fx/         coinbase, coingecko, static, cache + fallback chain
    lightning/  trait, BOLT11 decoding, lnbits
    bitnob/     signed client, payout adapter (bank + mobile money), rate source
    payout/     trait + registry, lightning_address (Tapnob), ngn_mock, tando_mock, mobile_money_mock
  domain/       Currency, money units, PaymentStatus state machine, Recipient and Rail
  db.rs         models and queries; `transition()` is the only way a status changes
  security.rs   HMAC signing/verification, account hashing
  ratelimit.rs  per-user, per-IP and sign-up limits
  bin/bitnob_sandbox.rs   local stand-in for Bitnob
  bin/lnbits_sandbox.rs   local stand-in for LNbits
migrations/     SQLite schema (0001 core, 0002 users, recovery, refunds)
tests/          end-to-end flows through the router (test-only Lightning node), the Bitnob
                sandbox, and opt-in live tests
```

## Not done yet

- **Real KES provider.** Tando is still a mock. Bitnob's `KE` corridor could replace it but has not been run live.
- **Bitnob live verification.** Everything Bitnob-related is tested against fakes and the local stand-in only. The real API still needs the client secret, a funded balance and a check of the webhook signature encoding (the adapter assumes hex).
- **Daily NGN limits.** Only a per-payout maximum exists (the mock cap and Bitnob's limits), not a daily total.
- **Token recovery.** A lost user token cannot be recovered.
- **Hardening:** keyed hashing for stored account numbers (`security::hash_account`).
