# SatsBridge backend

Rust API for Lightning-settled cross-border payments: [axum](https://github.com/tokio-rs/axum) 0.8, tokio, sqlx with SQLite (migrations run on startup), reqwest with rustls.

For what is built and what has been verified against live services, see [`../docs/implementation-status.md`](../docs/implementation-status.md).

## Run

Requires Rust 1.88 or newer.

```bash
cp .env.example .env
cargo run            # http://localhost:8080/api/v1/health
cargo test           # 40 unit + end-to-end tests (no network needed)
```

Docker: `docker build -t satsbridge-api . && docker run -p 8080:8080 --env-file .env -v satsbridge-data:/data satsbridge-api`

With the defaults in `.env.example`, everything runs locally with mocks. The exception is FX, which fetches live rates from Coinbase and CoinGecko. For fully offline runs, set `FX_PROVIDERS=static`.

## Providers

Every external system sits behind a trait. A setting in `.env` chooses between the mock and the real implementation.

| Trait | Mock (default) | Real | Select with |
|---|---|---|---|
| `FxProvider` | `StaticFxProvider` | Coinbase → CoinGecko chain, cached | `FX_PROVIDERS` |
| `LightningAdapter` | `MockLightningAdapter` | `LnbitsAdapter` | `LIGHTNING_PROVIDER=lnbits` |
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

- **Receiving:** invoices are created with `LNBITS_INVOICE_KEY`.
- **Settlement:** always confirmed by asking LNbits (`GET /api/v1/payments/{hash}`), in two ways:
  - The **settlement watcher** checks unpaid invoices every `LIGHTNING_POLL_SECS` (default 5). It also expires invoices that run out unpaid. It works without a public URL.
  - `POST /webhooks/lnbits` receives LNbits's invoice webhook (set `LNBITS_WEBHOOK_URL`). The webhook is unsigned, so it only triggers the same check.

  Both paths share an event id, so an invoice settles exactly once.
- **Sending** (Tapnob payouts only): uses `LNBITS_ADMIN_KEY`. A payment counts as failed only when LNbits says it failed. A timeout or an in-flight payment leaves the payout `PAYOUT_PENDING`, and it is never refunded.
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
- **Requirement:** needs `LIGHTNING_PROVIDER=lnbits` with an admin key. The mock node only pretends to pay.

## Configuration

Every variable is documented in [`.env.example`](.env.example). The main ones:

| Variable | Purpose | Default |
|---|---|---|
| `DATABASE_URL` | SQLite path | `sqlite://satsbridge.db` |
| `API_KEY` | Bearer token for protected routes; empty disables auth | — |
| `LIGHTNING_WEBHOOK_SECRET`, `PROVIDER_WEBHOOK_SECRET` | HMAC secrets for the generic webhooks | **required** |
| `QUOTE_TTL_SECS`, `INVOICE_EXPIRY_SECS` | Quote lock and invoice lifetime | 120, 600 |
| `FEE_BPS`, `FLAT_FEE_SATS` | SatsBridge fee | 100 (1%), 0 |
| `FX_PROVIDERS` | `coinbase,coingecko[,static]` | `coinbase,coingecko` |
| `LIGHTNING_PROVIDER` | `mock` \| `lnbits` (+ `LNBITS_URL`, `LNBITS_INVOICE_KEY`, `LNBITS_ADMIN_KEY`, `LNBITS_WEBHOOK_URL`) | `mock` |
| `LIGHTNING_POLL_SECS` | Settlement watcher interval; 0 disables it | 5 |
| `NGN_BANK_PROVIDER` | `mock` \| `bitnob` (+ `BITNOB_*`) | `mock` |
| `NGN_MAX_PAYOUT_MINOR` | Per-payout NGN cap in kobo | 50000000 (₦500k) |
| `TAPNOB_DOMAINS` | Lightning Address domains; empty disables the rail | `tapnob.io` |
| `DEV_ROUTES` | Mounts the `/dev/*` simulation routes. **Never in production** | `false` |

## Conventions

- **Amounts** are integers in minor units. `GET /currencies` gives each currency's `exponent`: XOF/XAF/GNF = 0, KES/NGN = 2. Bitcoin amounts are in sats.
- **Fees** are taken in sats. `fee_source_amount` shows them in the sender's currency.
- **Exactly one payout per payment:** repeated webhook events are discarded, status only changes if it's still in the expected state, and the database allows one payout row per payment. The payment ID is the provider reference.
- **Webhook authentication:**
  - `/webhooks/lightning` and `/webhooks/provider`: `X-SatsBridge-Signature: <hex HMAC-SHA256 of raw body>`
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
| POST | `/webhooks/lightning` | `{event_id, payment_hash, amount_sats}`, HMAC-signed |
| POST | `/webhooks/provider` | `{event_id, provider_reference, status: success\|failed\|reversed, reason?}`, HMAC-signed |
| POST | `/webhooks/bitnob` | Bitnob payout callbacks |
| POST | `/webhooks/lnbits` | LNbits invoice webhook |
| POST | `/dev/payments/{id}/settle` | Dev only: simulate Lightning settlement |
| POST | `/dev/payments/{id}/payout` | Dev only: `{status}` simulates a provider callback |

Recipients:

```json
{ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }
{ "rail": "NG_BANK_TRANSFER", "bank_code": "058", "account_number": "0123456789", "resolution_id": "res_..." }
{ "rail": "LIGHTNING_ADDRESS", "address": "ada@tapnob.io" }
```

## Demos

Run with `DEV_ROUTES=true` and the mock Lightning node. `jq` is used to read responses.

### XOF → NGN bank account (mock NGN rail)

```bash
API=localhost:8080/api/v1; J='content-type: application/json'
Q=$(curl -s -XPOST $API/quotes -H "$J" -d '{"source_currency":"XOF","destination_currency":"NGN","amount":100000}' | jq -r .id)
R=$(curl -s -XPOST $API/recipients/resolve -H "$J" -d '{"currency":"NGN","bank_code":"058","account_number":"0123456789"}' | jq -r .resolution_id)
P=$(curl -s -XPOST $API/payments -H "$J" -d "{\"quote_id\":\"$Q\",\"recipient\":{\"rail\":\"NG_BANK_TRANSFER\",\"bank_code\":\"058\",\"account_number\":\"0123456789\",\"resolution_id\":\"$R\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay | jq .payment_request
curl -s -XPOST $API/dev/payments/$P/settle
curl -s -XPOST $API/dev/payments/$P/payout -H "$J" -d '{"status":"success"}'
curl -s $API/payments/$P | jq '{status, expected_payout, payout, events: [.events[].event_type]}'
```

### XOF → NGN via a Tapnob Lightning Address

To move real sats, use `LIGHTNING_PROVIDER=lnbits` with a funded admin key. On the mock node, the lookup and invoice are real but the payment is simulated.

```bash
Q=$(curl -s -XPOST $API/quotes -H "$J" -d '{"source_currency":"XOF","destination_currency":"NGN","amount":100000,"rail":"LIGHTNING_ADDRESS"}' | jq -r .id)
P=$(curl -s -XPOST $API/payments -H "$J" -d "{\"quote_id\":\"$Q\",\"recipient\":{\"rail\":\"LIGHTNING_ADDRESS\",\"address\":\"<name>@tapnob.io\"}}" | jq -r .id)
curl -s -XPOST $API/payments/$P/pay | jq .payment_request
curl -s -XPOST $API/dev/payments/$P/settle      # with LNbits, pay the invoice instead; the watcher settles it
curl -s $API/payments/$P | jq '{status, payout}'
```

## Layout

```
src/
  api/          handlers, routing, API-key middleware, webhooks, dev simulation routes
  services/     quotes (FX + fee math), recipients (NGN resolution), payments (orchestration),
                settlement (invoice watcher)
  adapters/
    fx/         coinbase, coingecko, static, cache + fallback chain
    lightning/  trait + mock, lnbits
    payout/     trait + registry, bitnob, lightning_address (Tapnob), ngn_mock, tando_mock
  domain/       Currency, money units, PaymentStatus state machine, Recipient and Rail
  db.rs         models and queries; `transition()` is the only way a status changes
  security.rs   HMAC signing/verification, account hashing
migrations/     SQLite schema
tests/          end-to-end flows through the router
```

## Not done yet

- **Payout retries:** an outbox/retry worker for payout submission. Bitnob's handling of a reused `reference` after an interrupted payout is unknown, so this needs reconciliation first.
- **Bitnob rates:** use Bitnob's own NGN rate when quoting.
- **More corridors:** Bitnob also pays out KES, XOF (CI/SN/NE) and XAF (CM), so it could cover the remaining corridors and replace the Tando mock.
- **LNURL for senders:** serve LNURL-pay to senders (FR-06). Today we only *consume* LNURL, for Tapnob payouts.
- **Refunds:** carry out refunds (`REFUND_PENDING → REFUNDED`).
- **Auth and limits:** per-user auth, scoping `/transactions` to the caller, and rate limiting.
- **Hardening:** keyed hashing for stored account numbers (`security::hash_account`), and an explicit CORS allowlist.
