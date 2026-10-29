# SatsBridge

Cross-border payments between African currencies, settled over Bitcoin's Lightning Network.

A sender picks a currency pair and an amount, gets a quote, and pays a Lightning invoice. SatsBridge then pays the recipient in their local currency through a local payout rail. Bitcoin is the settlement layer underneath. The sender and recipient only ever see their own currencies.

**Currencies:** XOF, XAF, GNF, KES, NGN
**Status:** hackathon MVP. The backend is built and tested; there is no frontend in this repo. The API has been run end to end with the local stand-ins. Sandbox and demo use only. See [Implementation status](docs/implementation-status.md).

## How a payment flows

```
Quote (live BTC rates) → Payment → Lightning invoice (LNbits) → Settled → Local payout → Completed
```

| Piece | Implementation |
|---|---|
| Exchange rates | Bitnob's own payout rate when enabled, then Coinbase, then CoinGecko; XOF and XAF from the euro peg |
| Lightning | LNbits (required; there is no mock node) |
| NGN bank payouts | Bitnob (NIBSS instant transfer), or a mock |
| Mobile money (KES, XOF, XAF) | Bitnob per country, or mocks |
| NGN via Lightning Address | Tapnob (`name@tapnob.io`) |
| Senders | Anonymous per-user bearer tokens; LNURL-pay QR codes; refunds over Lightning |
| Operations | Admin API, payout recovery worker, rate limiting |

## Repository

| Path | Contents |
|---|---|
| [`backend/`](backend/) | Rust API (axum, tokio, sqlx/SQLite) and a local Bitnob stand-in. Setup, configuration, API reference and demo scripts are in the [backend README](backend/README.md). |
| [`docs/implementation-status.md`](docs/implementation-status.md) | What is built, what has been verified against live services, and what's left |
| [`docs/prd-addendum-ngn-payout.md`](docs/prd-addendum-ngn-payout.md) | Requirements for Naira payouts |

## Quick start

Requires Rust 1.88 or newer.

```bash
cd backend
cp .env.example .env   # set LNBITS_URL and LNBITS_INVOICE_KEY (required)
cargo run              # http://localhost:8080/api/v1/health
cargo test             # 71 unit and end-to-end tests (no credentials needed)
```

Payout rails default to mocks. For real Bitnob payouts set `BITNOB_COUNTRIES` (e.g. `NG`) with its credentials, or run the local stand-ins (`cargo run --bin bitnob_sandbox` and `cargo run --bin lnbits_sandbox`) to run the whole product without any external account. See the [backend README](backend/README.md#bitnob-bank-transfer-and-mobile-money).

## Disclaimer

This is a technical demonstration. It is not a licensed financial service. Moving money across borders, custody and foreign exchange are regulated differently in each country. Production use needs licensed partners, KYC/AML controls and legal review.
