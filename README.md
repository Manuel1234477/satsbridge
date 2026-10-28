# SatsBridge

Cross-border payments between African currencies, settled over Bitcoin's Lightning Network.

A sender picks a currency pair and an amount, gets a quote, and pays a Lightning invoice. SatsBridge then pays the recipient in their local currency through a local payout rail. Bitcoin is the settlement layer underneath. The sender and recipient only ever see their own currencies.

**Currencies:** XOF, XAF, GNF, KES, NGN
**Status:** hackathon MVP. The backend is built; the frontend hasn't been started. Sandbox and demo use only. See [Implementation status](docs/implementation-status.md).

## How a payment flows

```
Quote (live BTC rates) → Payment → Lightning invoice (LNbits) → Settled → Local payout → Completed
```

| Piece | Implementation |
|---|---|
| Exchange rates | Coinbase (primary) → CoinGecko (fallback); XOF and XAF from the euro peg |
| Lightning | LNbits (required; there is no mock node) |
| NGN bank payouts | Bitnob (NIBSS instant transfer), or a mock |
| NGN via Lightning Address | Tapnob (`name@tapnob.io`) |
| KES payouts | Tando (mock for now) |

## Repository

| Path | Contents |
|---|---|
| [`backend/`](backend/) | Rust API (axum, tokio, sqlx/SQLite). Setup, configuration, API reference and demo scripts are in the [backend README](backend/README.md). |
| [`docs/implementation-status.md`](docs/implementation-status.md) | What is built, what has been verified against live services, and what's left |
| [`docs/prd-addendum-ngn-payout.md`](docs/prd-addendum-ngn-payout.md) | Requirements for Naira payouts |

## Quick start

Requires Rust 1.88 or newer.

```bash
cd backend
cp .env.example .env   # set LNBITS_URL and LNBITS_INVOICE_KEY (required)
cargo run              # http://localhost:8080/api/v1/health
cargo test             # 44 unit + end-to-end tests (no credentials needed)
```

Payout providers default to mocks. For real Naira bank payouts, set `NGN_BANK_PROVIDER=bitnob` with its credentials. See the [backend README](backend/README.md#configuration).

## Disclaimer

This is a technical demonstration. It is not a licensed financial service. Moving money across borders, custody and foreign exchange are regulated differently in each country. Production use needs licensed partners, KYC/AML controls and legal review.
