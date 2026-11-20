# StellarBridge web app (Next.js)

Sender web app for StellarBridge: pick the currency you pay in and the one they receive, get a live quote from the Stellar DEX, confirm the recipient (M-Pesa, mobile money or bank), pay by bank transfer or mobile money, and follow the payment through its settlement on Stellar to the receipt. Built with Next.js 16 (App Router), TypeScript, Tailwind CSS v4, TanStack Query and Zod.

**Status:** hackathon MVP. No collection or payout provider is wired in the backend yet, so no real money is moved. This is a technical demonstration and not a licensed financial service.

## What is built and verified

| Area | Built | Verified by |
|---|---|---|
| Send screen: any of NGN/KES/GHS/XOF/XAF/GNF in, any other out, swap, live quotes (400 ms debounce), amount in either field, payout method selector, quote countdown, auto-refresh on expiry | Yes | Walked by hand against the backend on Stellar testnet |
| Live rate: e.g. "1 KES ≈ x NGN" from the latest Stellar DEX quote, the DEX path (`KES → USD → NGN`), guaranteed-amount note | Yes | Walked by hand |
| KES M-Pesa, mobile money (country, provider, phone) and NGN bank (resolve + "Confirm account holder") recipient forms | Yes | M-Pesa in browser tests; NGN bank form via the refund path in unit tests |
| Review, then `POST /payments`, then `POST /payments/{id}/pay`. A failed `/pay` retries on the same payment, never a second one | Yes | Walked by hand |
| Payment page: bank transfer details (NGN) or mobile money / M-Pesa Paybill details with a reference (other currencies), copy buttons, countdown, polling every 3 s | Yes | Walked by hand against the backend |
| Tracker (transfer received, settled on Stellar, paying, delivered); "On Stellar" panel with each transaction (mint + convert, burn, reverse), delivered amount, ledger and explorer link | Yes | Walked by hand |
| Receipt on COMPLETED: amounts, fee in the sender's currency, rate, Stellar route, recipient, reference | Yes (no print stylesheet) | Walked by hand |
| Refund: automatic to the sender's account in their currency; status shown while in flight; if the source account is unknown or the refund failed, an account form for that currency (bank, M-Pesa or mobile money) | Yes | Walked by hand |
| Reversal: COMPLETED that becomes FAILED with payout `reversed` shows "Payment reversed", then the refund | Yes | Walked by hand |
| History, API client (Zod validation, error mapping, 401 retry), integer money | Yes | Vitest |

Unit tests: Vitest. There are no browser tests; the flow was walked by hand against the real backend on Stellar testnet.

## What is not built or not verified

- **Internationalization (English and French): not built.**
- **Accessibility audit: not done.** Focus styles and labels are in place, but there was no axe run and no keyboard-only pass.
- **Refund account form, history page and expired-deposit path: no browser tests.**
- **Receipt is print-to-PDF via `window.print()` only,** with no print stylesheet.
- **Not in CI.**

## Setup

```bash
cd web
cp .env.example .env.local   # then adjust
npm install --legacy-peer-deps
npm run dev                  # http://localhost:3000
```

`--legacy-peer-deps` is needed because `@testing-library/react` 14 declares a React 18 peer.

### Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `NEXT_PUBLIC_API_URL` | `http://localhost:8080/api/v1` | Backend base URL |

Build-time variables: Next inlines `NEXT_PUBLIC_*` values, so restart the dev server after changing them.

## Running against the real backend

```bash
cd ../backend
cp .env.example .env
cargo run --bin stellar_setup   # testnet accounts and DEX liquidity; paste the printed keys into .env
cargo run                       # serves http://localhost:8080
```

No network? Set `STELLAR_LEDGER=simulated` in the backend's `.env` and skip `stellar_setup`.

Then run the frontend with `npm run dev`. The backend's `.env.example` already allows `http://localhost:3000` via `CORS_ALLOWED_ORIGINS`.

## Scripts

| Command | What it does |
|---|---|
| `npm run dev` | Dev server on port 3000 |
| `npm run build` | Production build |
| `npm run typecheck` | `tsc --noEmit` (strict) |
| `npm run lint` | ESLint CLI, `--max-warnings 0` |
| `npm test` | Vitest, once with `npm test -- --run` |


The demo script is in [`../docs/demo-script.md`](../docs/demo-script.md).

## Layout

```
src/
  app/                 routes: / (send, ?from=<id> to prefill), /payments/[id], /history
  components/          LiveRatePanel, ui/ (AppShell, AmountInput, CurrencySheet, RailSelector, StatusPill, ...)
  features/
    send/              SendPage, ReviewPage, SendWizard
    recipient/         KePhoneRecipient, MobileMoneyRecipient, NgBankRecipient (also the refund account form)
    payment/           PaymentView (every status): bank details, tracker, Stellar trail, refund
    history/           HistoryList
  hooks/               useQuotePreview, usePayment, useCountdown, useDebounced, useCurrencies, ...
  lib/
    api/               client (auth, retry), schemas (Zod), index (endpoints), errors
    money.ts           exact integer money formatting and parsing
    payment-status.ts  status to tracker step, polling interval
    rails.ts           payout method labels
```

## Design notes

- The client never computes money. It displays server amounts; the only arithmetic is formatting and a display countdown.
- The sender cannot submit `POST /payments` on an expired quote. The UI blocks it and asks them to go back for a fresh rate.
- The token is kept in `localStorage` and sent as a Bearer header, never in a URL. A 401 clears it, creates a new anonymous user, and retries once.
- Account numbers are kept in component memory only until payment is created, and are shown masked afterwards.
- Stellar transaction links go to stellar.expert (testnet); the simulated ledger shows the hash only.
