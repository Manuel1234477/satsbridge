# SatsBridge web app (Next.js)

Sender web app for SatsBridge: send naira, get a live quote from the Stellar DEX, confirm the recipient (M-Pesa, mobile money or bank), pay by bank transfer, and follow the payment through its settlement on Stellar to the receipt. Built with Next.js 16 (App Router), TypeScript, Tailwind CSS v4, TanStack Query and Zod.

**Status:** hackathon MVP, demo only. No real money is moved. This is a technical demonstration and not a licensed financial service.

## What is built and verified

| Area | Built | Verified by |
|---|---|---|
| Send screen: naira in (the only currency with a collection rail), KES/GHS/XOF/XAF/GNF out, live quotes (400 ms debounce), amount in either field, payout method selector, quote countdown, auto-refresh on expiry | Yes | Browser tests (mock and real backend on Stellar testnet) |
| Live rate: "1 NGN ≈ x KES" from the latest Stellar DEX quote, the DEX path (`NGN → USD → KES`), guaranteed-amount note | Yes | Browser test against the real backend |
| KES M-Pesa, mobile money (country, provider, phone) and NGN bank (resolve + "Confirm account holder") recipient forms | Yes | M-Pesa in browser tests; NGN bank form via the refund path in unit tests |
| Review, then `POST /payments`, then `POST /payments/{id}/pay`. A failed `/pay` retries on the same payment, never a second one | Yes | Browser tests |
| Payment page: bank transfer details (account number, bank, account name, exact amount, copy buttons), deposit countdown, polling every 3 s | Yes | Browser tests (mock and real backend) |
| Tracker (transfer received, settled on Stellar, paying, delivered); "On Stellar" panel with each transaction (mint + convert, burn, reverse), delivered amount, ledger and explorer link | Yes | Browser test against the real backend on testnet |
| Receipt on COMPLETED: amounts, naira fee, rate, Stellar route, recipient, reference | Yes (no print stylesheet) | Browser tests |
| Refund: automatic to the sender's account; status shown while in flight; if the source account is unknown or the refund failed, an NGN bank account form | Yes | Browser test (automatic); unit test (account form path in the mock) |
| Reversal: COMPLETED that becomes FAILED with payout `reversed` shows "Payment reversed", then the refund | Yes | Browser test |
| Dev panels (`NEXT_PUBLIC_DEV_TOOLS=true`, mock providers only): simulate the bank transfer, the payout result and the refund result | Yes | Browser test against the real backend |
| History, API client (Zod validation, error mapping, 401 retry), integer money | Yes | Vitest |
| Mock API mode: same routes and shapes, time-derived status progression, magic values | Yes | Vitest (mock contract), Playwright |

Unit tests: 37 Vitest tests. Browser tests: 3 Playwright tests in mock mode; the happy path was also walked against the real backend on Stellar testnet (screens and explorer links checked, no console errors).

## What is not built or not verified

- **Internationalization (English and French): not built.**
- **Accessibility audit: not done.** Focus styles and labels are in place, but there was no axe run and no keyboard-only pass.
- **Refund account form, history page and expired-deposit path: no browser tests.**
- **Receipt is print-to-PDF via `window.print()` only,** with no print stylesheet.
- **Not in CI.**

## Two modes

The app runs in one of two modes, chosen by `NEXT_PUBLIC_MOCK_API`. The default is `false`.

| Mode | `NEXT_PUBLIC_MOCK_API` | Needs | Rates and payments |
|---|---|---|---|
| **Real backend** (default) | `false` | The Rust backend on `http://localhost:8080`, set up on Stellar testnet (or with `STELLAR_LEDGER=simulated`) | Live quotes from the Stellar DEX. Settlement on Stellar testnet |
| **Mock** | `true` | Nothing | An in-browser stand-in with the same routes and shapes. Fixed demo rates. No money moves |

Mock mode is for demos and tests when the backend or the network is unavailable. Its rates are fixed demo values and its Stellar transactions are made up, so "Live" in mock mode does not mean a live market rate.

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
| `NEXT_PUBLIC_MOCK_API` | `false` | `true` serves every request from the in-browser mock. No backend needed |
| `NEXT_PUBLIC_MOCK_TIME_SCALE` | `1` | Mock only. Multiplies every mock timing; `0.2` runs five times faster |
| `NEXT_PUBLIC_DEV_TOOLS` | `false` | Shows the dev panels that simulate the bank transfer, payout and refund (mock providers only) |

Build-time variables: Next inlines `NEXT_PUBLIC_*` values, so restart the dev server after changing them.

## Running against the real backend

```bash
cd ../backend
cp .env.example .env
cargo run --bin stellar_setup   # testnet accounts and DEX liquidity; paste the printed keys into .env
cargo run                       # serves http://localhost:8080
```

No network? Set `STELLAR_LEDGER=simulated` in the backend's `.env` and skip `stellar_setup`.

Then run the frontend with `NEXT_PUBLIC_MOCK_API=false` (and `NEXT_PUBLIC_DEV_TOOLS=true` to simulate the bank transfer). The backend's `.env.example` already allows `http://localhost:3000` via `CORS_ALLOWED_ORIGINS`.

## Mock mode

`NEXT_PUBLIC_MOCK_API=true npm run dev` answers the same routes with the same JSON shapes; the same Zod schemas validate them. Payment progress is computed from timestamps, so reloading keeps the state. State is stored in `localStorage` under `satsbridge.mock.v2`. The bank transfer "arrives" 8 seconds after the details are shown (or at once with the dev button).

Magic values (the recipient's phone or account number, and amounts):

| Input | Result |
|---|---|
| Any valid number | Settled, delivered, receipt |
| Number ending `0000` | Payout fails, conversion reversed on Stellar, naira refunded |
| Number ending `1111` | Delivered, then reversed by the provider; refund follows |
| Number ending `2222` | The transfer never arrives; the payment expires |
| NGN account ending `9999` | Account check fails (`recipient_not_found`) |
| Source amount ₦777 | Quote expires after 5 seconds (scaled by time scale) |

## Scripts

| Command | What it does |
|---|---|
| `npm run dev` | Dev server on port 3000 |
| `npm run build` | Production build |
| `npm run typecheck` | `tsc --noEmit` (strict) |
| `npm run lint` | ESLint CLI, `--max-warnings 0` |
| `npm test` | Vitest, once with `npm test -- --run` |
| `npm run e2e` | Playwright; starts its own mock dev server on port 3100 |

Playwright uses the system Chrome at `/usr/bin/google-chrome`. Set `PLAYWRIGHT_CHROME_PATH` to use another binary.

The demo script is in [`../docs/demo-script.md`](../docs/demo-script.md).

## Layout

```
src/
  app/                 routes: / (send, ?from=<id> to prefill), /payments/[id], /history
  components/          LiveRatePanel, ui/ (AppShell, AmountInput, CurrencySheet, RailSelector, StatusPill, ...)
  features/
    send/              SendPage, ReviewPage, SendWizard
    recipient/         KePhoneRecipient, MobileMoneyRecipient, NgBankRecipient (also the refund account form)
    payment/           PaymentView (every status): bank details, tracker, Stellar trail, refund, dev panels
    history/           HistoryList
  hooks/               useQuotePreview, usePayment, useCountdown, useDebounced, useCurrencies, ...
  lib/
    api/               client (auth, retry), schemas (Zod), index (endpoints), errors, mock
    money.ts           exact integer money formatting and parsing
    payment-status.ts  status to tracker step, polling interval
    rails.ts           payout method labels
e2e/                   Playwright specs and helpers
```

## Design notes

- The client never computes money. It displays server amounts; the only arithmetic is formatting and a display countdown.
- The sender cannot submit `POST /payments` on an expired quote. The UI blocks it and asks them to go back for a fresh rate.
- The token is kept in `localStorage` and sent as a Bearer header, never in a URL. A 401 clears it, creates a new anonymous user, and retries once.
- Account numbers are kept in component memory only until payment is created, and are shown masked afterwards.
- Stellar transaction links go to stellar.expert (testnet); the simulated ledger and mock mode show the hash only.
