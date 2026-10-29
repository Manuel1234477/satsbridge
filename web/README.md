# SatsBridge frontend

Sender web app for SatsBridge: pick a currency pair, get a live quote, confirm the recipient, pay a Lightning invoice, and follow the payment to receipt. Built with Next.js 16 (App Router), TypeScript, Tailwind CSS v4, TanStack Query and Zod.

**Status:** hackathon MVP, demo only. No real money is moved. This is a technical demonstration and not a licensed financial service.

## What is built and verified

| Area | Built | Verified by |
|---|---|---|
| Send screen: live quotes (400 ms debounce), amount in either field, swap, payout method selector, quote countdown, auto-refresh on expiry, continue blocked on an expired quote | Yes | Browser test (happy path) |
| Live rate: "1 XOF ≈ x NGN" from the latest quote, rate source, "updated Xs ago" from `rate_as_of`; quote refreshed every 30 s while the tab is visible and on expiry; Live/Stale pill (Stale after 2 min or a failed request); Try again on `provider_unavailable` and `rate_limited` | Yes | Unit test (freshness rule). Not run against the real backend |
| NGN bank recipient: bank search, 10-digit account, resolve, "Confirm account holder" step, resolution cleared when bank or account changes | Yes | Browser test (happy path) |
| Lightning Address and KES M-Pesa recipient forms | Yes | Not run in a browser |
| Review, then `POST /payments`, then `POST /payments/{id}/pay`. A failed `/pay` retries on the same payment, never a second one | Yes | Browser test (happy path) |
| Payment page: QR (`lightning:` URI), copy invoice, open in wallet, copy LNURL, invoice countdown, polling every 3 s (stops on EXPIRED/REFUNDED; COMPLETED polled every 30 s to catch reversals) | Yes | Browser tests |
| Tracker (received, converting, paying, delivered), raw status in Details, event timeline | Yes | Browser tests |
| Receipt on COMPLETED, "Download receipt" via `window.print()` | Yes (no print stylesheet) | Browser test checks the heading and masked account |
| Refund: REFUND_PENDING prompt for an invoice for exactly `amount_sats`, `ln` prefix check, server's amount message shown, REFUNDED with `refunded_at` and `refund_sats` | Yes | Browser test |
| Reversal: COMPLETED that becomes FAILED with payout `reversed` shows "Payment reversed"; refund path as above | Yes | Browser test |
| Expired invoice: explanation and "Start a new quote" (prefills amounts only) | Yes | Not run in a browser |
| Failed payout (FAILED without reversal) | Yes | Browser test (refund path) |
| Dev "Simulate payout" panel (`NEXT_PUBLIC_DEV_TOOLS=true`, mock provider only) | Yes | Not run in a browser |
| History: list from `/transactions`, status badges, empty and error states | Yes | Not run in a browser |
| API client: Zod validation of every response, error mapping for all backend codes, one retry after 401 with a fresh anonymous user | Yes | Vitest (error mapping, mock payloads) |
| Money: integer minor units, exponents from `GET /currencies`, parse and format without floats | Yes | Vitest (9 tests) |
| Mock API mode (`NEXT_PUBLIC_MOCK_API=true`): same routes and shapes, time-derived status progression, magic values for failure, reversal, expiry and expired quote | Yes | Vitest (mock contract), Playwright |
| Disclaimer banner ("Demo mode", dismissable) | Yes | Visual check not done |

Unit tests: 22 Vitest tests (money, status-to-step mapping, error mapping, mock contract). Browser tests: 3 Playwright tests in Chrome, all against mock mode.

## What is not built or not verified

- **Real backend end to end: not run.** The backend needs LNbits. Nothing in the real-backend path was exercised in this work. Only the mock was tested in a browser.
- **Internationalization (English and French, language toggle): not built.** All strings are hardcoded English. There is no `src/i18n` directory.
- **Accessibility audit: not done.** Focus styles and labels are in place, but there was no axe run and no keyboard-only pass.
- **360 px, light and dark visual checks: not done.** Dark mode follows `prefers-color-scheme` only; there is no manual toggle. Layouts were not screenshotted.
- **shadcn/ui and React Hook Form: not used.** Forms are plain React state with inline validation.
- **Recipient prefill on "Start a new quote": not done.** The backend never returns the account, and the frontend must not store it, so only amounts and currencies are prefilled.
- **Refund invoice amount is not checked in the browser.** The backend checks that it matches `amount_sats` exactly and returns its message.
- **Offline UI: not built.** Network failures map to an "offline" message in the same error panels; there is no dedicated offline state.
- **Lightning Address and KES flows, expired-invoice path, history page, dev panel: no browser tests.**
- **Receipt is print-to-PDF via `window.print()` only,** with no print stylesheet.
- **Not in CI.** No pipeline runs these checks.

## Two modes

The app runs in one of two modes, chosen by `NEXT_PUBLIC_MOCK_API`. The default is `false`.

| Mode | `NEXT_PUBLIC_MOCK_API` | Needs | Rates and payments |
|---|---|---|---|
| **Real backend** (default) | `false` | The Rust backend on `http://localhost:8080`, with LNbits and its keys, and outbound internet for the rate provider | Live quotes from the backend. Real Lightning invoices |
| **Mock** | `true` | Nothing | An in-browser stand-in with the same routes and shapes. Fixed demo rates. No money moves |

Mock mode is for demos and tests when the backend, LNbits or the network is unavailable. Its rates are fixed demo values, so the "Live" label in mock mode does not mean a live market rate.

The backend refuses to start unless LNbits is reachable with valid keys. Its quotes also need the rate provider to be reachable. See [Running against the real backend](#running-against-the-real-backend).

## Setup

```bash
cd frontend
cp .env.example .env.local   # then adjust
npm install --legacy-peer-deps
npm run dev                  # http://localhost:3000
```

`--legacy-peer-deps` is needed because `@testing-library/react` 14 declares a React 18 peer.

### Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `NEXT_PUBLIC_API_URL` | `http://localhost:8080/api/v1` | Backend base URL |
| `NEXT_PUBLIC_MOCK_API` | `false` | `true` serves every request from the in-browser mock. No backend or LNbits needed |
| `NEXT_PUBLIC_MOCK_TIME_SCALE` | `1` | Mock only. Multiplies every mock timing; `0.2` runs five times faster |
| `NEXT_PUBLIC_DEV_TOOLS` | `false` | Shows the "Dev: simulate payout" panel on mock payouts |

Build-time variables: Next inlines `NEXT_PUBLIC_*` values, so restart the dev server after changing them.

## Running against the real backend

Requirements, all of which must be met for the backend to start and quote live rates:

- LNbits reachable at `LNBITS_URL`, with `LNBITS_INVOICE_KEY` and `LNBITS_ADMIN_KEY`. The backend checks these at startup.
- `PROVIDER_WEBHOOK_SECRET` set (the backend refuses to start without it).
- Outbound HTTPS to the rate provider (Coinbase by default). Without it, the backend cannot produce a live rate.

Not every machine meets these. The mock mode exists for the cases where they are not met.

```bash
cd ../backend
cp .env.example .env         # set LNBITS_URL and LNBITS_INVOICE_KEY
cargo run                    # serves http://localhost:8080
```

Then run the frontend with `NEXT_PUBLIC_MOCK_API=false`. The backend's `.env.example` already allows `http://localhost:3000` via `CORS_ALLOWED_ORIGINS`.

## Mock mode

`NEXT_PUBLIC_MOCK_API=true npm run dev` answers the same routes with the same JSON shapes; the same Zod schemas validate them. Payment progress is computed from timestamps, so reloading keeps the state. State is stored in `localStorage` under `satsbridge.mock.v1`.

Magic values (bank account numbers and amounts):

| Input | Result |
|---|---|
| Any valid 10-digit account, bank GTBank | Paid, delivered, receipt |
| Account ending `0000` | Payout fails, refund needed, refund to REFUNDED |
| Account ending `1111` | Delivered, then reversed; refund needed |
| Account ending `2222` | Invoice never paid, expires |
| Account ending `9999` | Account check fails (`recipient_not_found`) |
| Source amount `777` | Quote expires after 5 seconds (scaled by time scale) |

Refunds in mock mode check the BOLT11 amount, as the backend does: `lnbc<sats×10>n1…` is exact; any other amount is rejected with the server's message.

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
  components/          MoneyField, CurrencySelect, QuoteCard, LightningQr, Disclaimer
  features/
    send/              SendPage, ReviewPage, SendWizard
    recipient/         NgBankRecipient, LightningAddressRecipient, KePhoneRecipient
    payment/           PaymentView (every status), refund, dev panel
    history/           HistoryList
  hooks/               useQuotePreview, usePaymentDetails, useCountdown, useDebounced, ...
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
