# SatsBridge Frontend

A Next.js frontend for SatsBridge, a cross-border payments app for African currencies, settled over Bitcoin's Lightning Network.

**Status:** ✅ MVP complete — XOF ↔ NGN (bank + Lightning) and KES flows working end-to-end.

## Setup

```bash
cd frontend
cp .env.example .env.local
npm install
npm run dev
```

Visit `http://localhost:3000` (backend must be running on `http://localhost:8080/api/v1`).

## Environment variables

- `NEXT_PUBLIC_API_URL`: Backend API URL (default: `http://localhost:8080/api/v1`)
- `NEXT_PUBLIC_DEV_TOOLS`: Show dev-only UI (mock payout controls) (default: `false`)
- `NEXT_PUBLIC_MOCK_API`: Use mock API instead of real backend (default: `false`)

## Running against the backend

1. Start the backend: `cd backend && cargo run` (ensure `LNBITS_URL` is set in `.env`)
2. Start the frontend: `npm run dev`

## Features

### Core Flow
1. **Send** - Currency pair, amount, live quote (400ms debounce) with countdown and auto-refresh
2. **Recipient** - NGN bank (searchable picker + account resolution), Lightning Address, or KES M-Pesa
3. **Review** - Summary with recipient confirmation (required)
4. **Pay** - QR code, invoice copy, sats + fiat display, open-in-wallet link
5. **Track** - Real-time status polling, completion screen with amounts

### Additional
- **History** - Transaction list with status badges, click-through to receipts
- **Status display** - AWAITING_PAYMENT, PAYMENT_DETECTED, SETTLED, PAYOUT_PENDING, COMPLETED, FAILED, EXPIRED
- **Design** - Mobile-first, dark mode, WCAG AA, keyboard navigation, `prefers-reduced-motion` respected
- **Disclaimer** - Dismissable "demo mode" banner

## Mock API mode (Coming Soon)

To test without the real backend or LNbits:

```bash
NEXT_PUBLIC_MOCK_API=true npm run dev
```

This uses MSW (Mock Service Worker) to intercept API calls and simulate the full payment flow, including status transitions.

## Scripts

- `npm run dev` - Start dev server on port 3000
- `npm run build` - Build for production
- `npm run lint` - Run ESLint (must pass with zero warnings)
- `npm run typecheck` - Run TypeScript type checking (strict mode)
- `npm test` - Run tests with Vitest

All scripts must pass before commit.

## Architecture

```
src/
  app/                    - Routes: / (send), /history, /payments/[id]
  components/             - UI: CurrencySelector, MoneyInput, QuoteCard, QrCode, Disclaimer
  features/
    send/                 - SendPage, SendWizard, ReviewPage
    recipient/            - NgBankRecipient, LightningAddressRecipient, KePhoneRecipient
    payment/              - PaymentQr
  lib/
    api/                  - Client, schemas (Zod), error mapping
    money.ts              - Format/parse with exponent
    payment-status.ts     - Status → UI step mapping
  hooks/                  - useUser, useCurrencies, useQuote, usePayment, useCountdown, useRecipientResolution
  styles/globals.css      - Design tokens (calm fintech palette)
```

**No server routes, no database:** Client drives all state via TanStack Query + backend API.

## Key libraries

- **Next.js 16** (App Router, Turbopack) - Framework
- **TypeScript (strict mode)** - 100% type coverage
- **Tailwind CSS + @tailwindcss/forms** - Styling + design tokens
- **TanStack Query** - Server state, polling (3s for payments)
- **Zod** - Runtime validation of all API responses
- **React Hook Form** - Form state (ready for forms)
- **Lucide React** - Icons
- **qrcode.react** - Lightning QR codes
- **MSW** - Mock API (infrastructure ready)

## Design

- Mobile-first (390px breakpoint)
- Dark mode support
- Calm, trustworthy fintech aesthetic
- WCAG AA contrast
- Full keyboard navigation
- Respects `prefers-reduced-motion`

## Disclaimer

This is a technical demonstration. It is not a licensed financial service. See the backend README for the full disclaimer.
