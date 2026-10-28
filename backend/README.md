# SatsBridge backend

Rust API for Lightning-settled cross-border payments: [axum](https://github.com/tokio-rs/axum) 0.8, tokio, sqlx with SQLite (migrations run on startup), reqwest with rustls.

For what is built and what has been verified against live services, see [`../docs/implementation-status.md`](../docs/implementation-status.md).

## Run

Requires Rust 1.88 or newer.

```bash
cp .env.example .env # then set LNBITS_URL and LNBITS_INVOICE_KEY (required)
cargo run            # http://localhost:8080/api/v1/health
cargo test           # 44 unit + end-to-end tests (no network or credentials needed)
```

Docker: `docker build -t satsbridge-api . && docker run -p 8080:8080 --env-file .env -v satsbridge-data:/data satsbridge-api`

Lightning is always real: SatsBridge needs an LNbits wallet and checks its keys at startup, refusing to start if they fail. Payout providers default to mocks (`NGN_BANK_PROVIDER=mock`, Tando). FX fetches live rates from Coinbase and CoinGecko.

### Live checks

Two opt-in tests run against real services. They are skipped when the variables aren't set:

```bash
# Read-only: Bitnob credentials (sandbox/production) and bank list. Uses BITNOB_* from .env.
cargo test --test bitnob_live -- --nocapture

# Receives and sends 21 sats on a real LNbits (needs two wallets on the same instance).
LNBITS_TEST_URL=... LNBITS_TEST_INVOICE_KEY=... LNBITS_TEST_ADMIN_KEY=... \
LNBITS_TEST_PAYEE_INVOICE_KEY=... cargo test --test lnbits_live -- --nocapture
```

## Providers

Every external system sits behind a trait. Lightning is LNbits only. For payouts, a setting in `.env` chooses between a mock and the real provider.

| Trait | Mock (default) | Real | Select with |
|---|---|---|---|
| `FxProvider` | `StaticFxProvider` | Coinbase → CoinGecko chain, cached | `FX_PROVIDERS` |
| `LightningAdapter` | none | `LnbitsAdapter` (required) | `LNBITS_URL`, `LNBITS_INVOICE_KEY` |
| `PayoutAdapter` NGN `NG_BANK_TRANSFER` | `MockNgnAdapter` | `BitnobAdapter` | `NGN_BANK_PROVIDER=bitnob` |
| `PayoutAdapter` NGN `LIGHTNING_ADDRESS` | — | `LightningAddressAdapter` (Tapnob) | `TAPNOB_DOMAINS` (empty disables it) |
| `PayoutAdapter` KES `KE_MOBILE_MONEY` | `MockTandoAdapter` | not built yet | — |

Payout adapters are registered by **currency and rail**. A quote can name a `rail`. Otherwise the currency's bank or mobile-money rail is used. A payment must use the same rail as its quote.

### FX rates

`FX_PROVIDERS` is tried in order for each currency. Each live source is fetched at most every `FX_CACHE_TTL_SECS`. If a source is down, its last good prices are served for up to `FX_MAX_AGE_SECS`. Each quote records `rate_source` and `rate_as_of`.

| Source | XOF/XAF | GNF | KES | NGN |
|---|---|---|---|---|
| Coinbase `/v2/exchange-rates` (no key) | via EUR peg | ✓ | ✓ | ✓ |
| CoinGecko `/simple/price` (optional key) | via EUR peg | ✗ | ✓ | ✓ |

XOF and XAF are always computed as BTC/EUR × 655.957 (fixed peg). These are reference rates, not prices you can execute at.

### Lightning (LNbits)

- **Startup check:** `GET /api/v1/wallet` with each key. SatsBridge refuses to start on a bad URL or key, or if `LNBITS_ADMIN_KEY` isn't actually an admin key. It logs the wallet name and balance.
- **Receiving:** invoices are created with `LNBITS_INVOICE_KEY`. If LNbits rejects `LNBITS_WEBHOOK_URL` (by default it only accepts domain names), the invoice is created without the webhook and the watcher settles it.
- **Settlement:** always confirmed by asking LNbits (`GET /api/v1/payments/{hash}`), in two ways:
  - The **settlement watcher** checks unpaid invoices every `LIGHTNING_POLL_SECS` (default 5). It also expires invoices that run out unpaid. It works without a public URL.
  - `POST /webhooks/lnbits` receives LNbits's invoice webhook (set `LNBITS_WEBHOOK_URL`). The webhook is unsigned, so it only triggers the same check.

  Both paths share an event id, so an invoice settles exactly once.
- **No fake settlement:** nothing settles a payment except LNbits reporting it paid. There is no shared-secret Lightning webhook and no dev "settle" route.
- **Sending** (Tapnob payouts only): uses `LNBITS_ADMIN_KEY`. A payment counts as failed only when LNbits says it failed. A timeout or an in-flight payment leaves the payout `PAYOUT_PENDING`, and it is never refunded. Retrying an already-paid invoice returns the existing payment and moves no money (verified on a real LNbits).
- **Invoice parsing:** invoices are decoded and their signatures verified. We skip the `lightning-invoice` crate's stricter checks, which reject invoices LNbits issues without a BOLT11 feature field.
- **Fees:** LNbits has no per-payment fee cap; the wallet's fee reserve applies.

### NGN: Bitnob bank transfer (`NG_BANK_TRANSFER`)

- **Account check:** before the sender confirms, the account name is looked up (`GET /api/payouts/account-lookup`). Banks come from `GET /api/payouts/banks/NG`. These are 6-digit NIP codes, e.g. Access = `000014`; the mock uses 3-digit codes such as `058`.
- **Payout:** after settlement, quote → initialize → **finalize (irreversible)**. The money is debited from your **pre-funded Bitnob balance** (`source: offchain`, asset `BITNOB_FROM_ASSET`). See [why funding is needed](../docs/implementation-status.md#7-why-a-funded-bitnob-account-is-needed).
- **Status:** arrives on `POST /webhooks/bitnob`, signed with HMAC-SHA512 in `x-bitnob-signature`. `payouts.withdrawal.success` → `COMPLETED`, `payouts.withdrawal.expired` → `FAILED`. `BITNOB_CALLBACK_URL` must be publicly reachable.
- **Amount:** the quoted Naira amount is guaranteed. Any difference from Bitnob's rate is absorbed and logged.
- **Limits:** minimum `BITNOB_NGN_MIN_PAYOUT_MINOR` (₦500), maximum `NGN_MAX_PAYOUT_MINOR`.

### NGN: Tapnob Lightning Address (`LIGHTNING_ADDRESS`)

- **Recipient:** gives `name@tapnob.io`. Tapnob converts the sats and pays the recipient's linked bank account.
- **Payout:** we resolve the address over LNURL-pay, fetch an invoice for the net sats, check its amount and metadata hash, and pay it from LNbits. The payout completes when the Lightning payment succeeds.
- **Amount:** Tapnob converts at its own rate, so quotes carry `payout_amount_guaranteed: false`.
- **Safety:** only `TAPNOB_DOMAINS` (and their subdomains) are contacted, and redirects aren't followed.
- **Requirement:** needs `LNBITS_ADMIN_KEY` and an LNbits wallet with outbound liquidity.

## Configuration

Every variable is documented in [`.env.example`](.env.example). The main ones:

| Variable | Purpose | Default |
|---|---|---|
| `DATABASE_URL` | SQLite path | `sqlite://satsbridge.db` |
| `API_KEY` | Bearer token for protected routes; empty disables auth | — |
| `PROVIDER_WEBHOOK_SECRET` | HMAC secret for `/webhooks/provider` | **required** |
| `QUOTE_TTL_SECS`, `INVOICE_EXPIRY_SECS` | Quote lock and invoice lifetime | 120, 600 |
| `FEE_BPS`, `FLAT_FEE_SATS` | SatsBridge fee | 100 (1%), 0 |
| `FX_PROVIDERS` | `coinbase,coingecko[,static]` | `coinbase,coingecko` |
| `LNBITS_URL`, `LNBITS_INVOICE_KEY` | LNbits instance and receive key | **required** |
| `LNBITS_ADMIN_KEY` | Spend key, needed for Tapnob payouts | — |
| `LNBITS_WEBHOOK_URL` | Public URL of `/webhooks/lnbits` | — |
| `LIGHTNING_POLL_SECS` | Settlement watcher interval; 0 is only allowed with a webhook URL | 5 |
| `NGN_BANK_PROVIDER` | `mock` \| `bitnob` (+ `BITNOB_*`; credentials checked at startup with `GET /api/whoami`, which also logs sandbox vs production) | `mock` |
| `NGN_MAX_PAYOUT_MINOR` | Per-payout NGN cap in kobo | 50000000 (₦500k) |
| `TAPNOB_DOMAINS` | Lightning Address domains; empty disables the rail | `tapnob.io` |
| `DEV_ROUTES` | Mounts the mock-payout callback simulator. **Never in production** | `false` |

## Conventions

- **Amounts** are integers in minor units. `GET /currencies` gives each currency's `exponent`: XOF/XAF/GNF = 0, KES/NGN = 2. Bitcoin amounts are in sats.
- **Fees** are taken in sats. `fee_source_amount` shows them in the sender's currency.
- **Exactly one payout per payment:** repeated webhook events are discarded, status only changes if it's still in the expected state, and the database allows one payout row per payment. The payment ID is the provider reference.
- **Webhook authentication:**
  - `/webhooks/provider`: `X-SatsBridge-Signature: <hex HMAC-SHA256 of raw body>`
  - `/webhooks/bitnob`: Bitnob's own HMAC-SHA512 signature
  - `/webhooks/lnbits`: confirmed with LNbits instead of a signature
- **Personal data:** full recipient details are never returned. Account numbers are masked.

## API (`/api/v1`)

| Method | Path | Notes |
|---|---|---|
| GET | `/health` | DB check and active Lightning node |
| GET | `/currencies` | Currencies, exponents, payout rails with provider, limits and `guaranteed_amount` |
| GET | `/payout-institutions?currency=NGN[&rail=...]` | Banks or wallets with codes |
| POST | `/recipients/resolve` | `{currency: "NGN", bank_code, account_number}` → `{resolution_id, account_name}` |
| POST | `/quotes` | `{source_currency, destination_currency, amount, amount_side?: source\|destination, rail?}` |
| GET | `/quotes/{id}` | |
| POST | `/payments` | `{quote_id, recipient}` (see below) |
| GET | `/payments/{id}` | Status, invoice, payout and audit trail (the receipt) |
| POST | `/payments/{id}/pay` | Returns the Lightning invoice; safe to repeat |
| GET | `/transactions?limit=50` | Most recent payments |
| POST | `/webhooks/provider` | `{event_id, provider_reference, status: success\|failed\|reversed, reason?}`, HMAC-signed |
| POST | `/webhooks/bitnob` | Bitnob payout callbacks |
| POST | `/webhooks/lnbits` | LNbits invoice webhook; always re-checked with LNbits |
| POST | `/dev/payments/{id}/payout` | Dev only: `{status}` simulates a callback from a **mock** payout provider. Refused for Bitnob and Tapnob |

Recipients:

```json
{ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }
{ "rail": "NG_BANK_TRANSFER", "bank_code": "058", "account_number": "0123456789", "resolution_id": "res_..." }
{ "rail": "LIGHTNING_ADDRESS", "address": "ada@tapnob.io" }
```

## Demos

Run with `DEV_ROUTES=true`. Each demo prints a real Lightning invoice. Pay it from any Lightning wallet, or from a second wallet on your LNbits instance. The settlement watcher picks up the payment within `LIGHTNING_POLL_SECS`. `jq` is used to read responses.

### XOF → NGN bank account (mock NGN rail)

```bash
API=localhost:8080/api/v1; J='content-type: application/json'
Q=$(curl -s -XPOST $API/quotes -H "$J" -d '{"source_currency":"XOF","destination_currency":"NGN","amount":100000}' | jq -r .id)
R=$(curl -s -XPOST $API/recipients/resolve -H "$J" -d '{"currency":"NGN","bank_code":"058","account_number":"0123456789"}' | jq -r .resolution_id)
P=$(curl -s -XPOST $API/payments -H "$J" -d "{\"quote_id\":\"$Q\",\"recipient\":{\"rail\":\"NG_BANK_TRANSFER\",\"bank_code\":\"058\",\"account_number\":\"0123456789\",\"resolution_id\":\"$R\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay | jq -r .payment_request   # pay this invoice
curl -s $API/payments/$P | jq -r .status                         # PAYOUT_PENDING once paid
curl -s -XPOST $API/dev/payments/$P/payout -H "$J" -d '{"status":"success"}'
curl -s $API/payments/$P | jq '{status, expected_payout, payout, events: [.events[].event_type]}'
```

### XOF → NGN via a Tapnob Lightning Address

Needs `LNBITS_ADMIN_KEY` and sats in the LNbits wallet. This sends a real payment to the address.

```bash
Q=$(curl -s -XPOST $API/quotes -H "$J" -d '{"source_currency":"XOF","destination_currency":"NGN","amount":100000,"rail":"LIGHTNING_ADDRESS"}' | jq -r .id)
P=$(curl -s -XPOST $API/payments -H "$J" -d "{\"quote_id\":\"$Q\",\"recipient\":{\"rail\":\"LIGHTNING_ADDRESS\",\"address\":\"<name>@tapnob.io\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay | jq -r .payment_request   # pay this invoice
curl -s $API/payments/$P | jq '{status, payout}'                # COMPLETED once SatsBridge pays the address
```

## Layout

```
src/
  api/          handlers, routing, API-key middleware, webhooks, dev simulation routes
  services/     quotes (FX + fee math), recipients (NGN resolution), payments (orchestration),
                settlement (invoice watcher)
  adapters/
    fx/         coinbase, coingecko, static, cache + fallback chain
    lightning/  trait, BOLT11 decoding, lnbits
    payout/     trait + registry, bitnob, lightning_address (Tapnob), ngn_mock, tando_mock
  domain/       Currency, money units, PaymentStatus state machine, Recipient and Rail
  db.rs         models and queries; `transition()` is the only way a status changes
  security.rs   HMAC signing/verification, account hashing
migrations/     SQLite schema
tests/          end-to-end flows through the router (test-only Lightning node); opt-in live tests
```

## Not done yet

- **Payout retries:** an outbox/retry worker for payout submission. Bitnob's handling of a reused `reference` after an interrupted payout is unknown, so this needs reconciliation first.
- **Bitnob rates:** use Bitnob's own NGN rate when quoting.
- **More corridors:** Bitnob also pays out KES, XOF (CI/SN/NE) and XAF (CM), so it could cover the remaining corridors and replace the Tando mock.
- **LNURL for senders:** serve LNURL-pay to senders (FR-06). Today we only *consume* LNURL, for Tapnob payouts.
- **Refunds:** carry out refunds (`REFUND_PENDING → REFUNDED`).
- **Auth and limits:** per-user auth, scoping `/transactions` to the caller, and rate limiting.
- **Hardening:** keyed hashing for stored account numbers (`security::hash_account`), and an explicit CORS allowlist.
