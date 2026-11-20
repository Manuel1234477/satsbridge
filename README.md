# StellarBridge

Cross-border payments from Nigerian naira to other African currencies, settled on the [Stellar](https://stellar.org) network.

A sender in Nigeria pays in naira by bank transfer. StellarBridge settles the conversion on Stellar and pays the recipient in their own currency: Kenyan shillings by M-Pesa, Ghanaian cedis, CFA francs (XOF, XAF) or Guinean francs by mobile money. Stellar is the settlement layer underneath. The sender and recipient only ever see their own currencies.

**Corridors:** NGN → KES, GHS, XOF, XAF, GNF
**Network:** Stellar testnet
**Status:** hackathon MVP, sandbox and demo use only. Settlement runs on real Stellar testnet. Bank collection and payouts are mocks behind provider interfaces. See [Implementation status](docs/implementation-status.md).

## How a payment flows

```
Quote (Stellar DEX) → Payment → Bank transfer in NGN → Settled on Stellar → Local payout → Completed
```

1. **Quote.** The rate comes from the Stellar DEX path NGN → USD → KES. The quoted payout is guaranteed.
2. **Pay in naira.** The sender gets a one-off account number and makes an ordinary bank transfer.
3. **Settle on Stellar.** One atomic transaction mints the deposit as an `NGN` token and converts it into `KES` with a strict-send path payment. Its minimum output is the quoted payout.
4. **Pay out.** The recipient is paid locally by M-Pesa, mobile money or bank. The `KES` tokens are then burned.
5. **If anything fails,** the conversion is reversed on Stellar and the naira goes back to the account it came from.

| Piece | Implementation |
|---|---|
| Settlement | Stellar testnet via Horizon: our issuer's fiat tokens, strict-send path payments over the DEX, transactions signed in Rust (`stellar-xdr`) |
| Liquidity | A market-maker account with two-sided offers against a `USD` bridge token, priced from Coinbase and CoinGecko (`stellar_setup`) |
| NGN collection | Per-payment virtual bank account (mock), deposit webhook |
| Payouts | M-Pesa, mobile money and NGN bank (for refunds), as mocks behind a provider interface |
| Senders | Anonymous per-user bearer tokens; automatic refunds to the source account |
| Operations | Admin API, recovery worker (Stellar transactions, payouts, refunds), rate limiting |

## Repository

| Path | Contents |
|---|---|
| [`backend/`](backend/) | Rust API (axum, tokio, sqlx/SQLite), the Stellar settlement layer, and the `stellar_setup` testnet tool. Setup, configuration, API reference and demos are in the [backend README](backend/README.md). |
| [`web/`](web/) | Next.js sender app: quote, pay by bank transfer, track the settlement on Stellar, get a receipt or refund. Runs against the backend or an in-browser mock. See the [web README](web/README.md). |
| [`docs/implementation-status.md`](docs/implementation-status.md) | What is built, what has been verified against live services, and what's left |

## Quick start

Requires Rust 1.88 or newer.

```bash
cd backend
cp .env.example .env
cargo run --bin stellar_setup   # creates testnet accounts, trustlines and DEX liquidity;
                                # paste the printed STELLAR_*_SECRET lines into .env
cargo run                       # http://localhost:8080/api/v1/health
cargo test                      # no network or credentials needed
```

No network? Set `STELLAR_LEDGER=simulated` in `.env` for an in-memory ledger, and skip `stellar_setup`.

Then the web app:

```bash
cd web
npm install --legacy-peer-deps
NEXT_PUBLIC_DEV_TOOLS=true npm run dev   # http://localhost:3000, against the backend
NEXT_PUBLIC_MOCK_API=true npm run dev    # or with no backend at all
```

## Disclaimer

This is a technical demonstration on Stellar **testnet**. Its tokens are test assets backed by nothing. It is not a licensed financial service. Moving money across borders, issuing fiat-backed tokens, custody and foreign exchange are regulated differently in each country. Production use needs licensed partners (Stellar anchors, banks, mobile money operators), KYC/AML controls and legal review.
