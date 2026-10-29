# SatsBridge Frontend

A Next.js frontend for SatsBridge, a cross-border payments app for African currencies, settled over Bitcoin's Lightning Network.

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

## Mock API mode

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
  app/           - Next.js App Router pages
  components/    - Reusable UI components
  features/      - Feature-specific components and hooks
  lib/
    api/         - API client and Zod schemas
    money.ts     - Money formatting utilities
  hooks/         - React hooks (queries, state)
  i18n/          - Internationalization (EN, FR)
  styles/        - Global styles and design tokens
```

## Key libraries

- **Next.js (App Router)** - Framework
- **TypeScript (strict)** - Type safety
- **Tailwind CSS** - Styling
- **shadcn/ui** - UI components (planned)
- **TanStack Query** - Server state & polling
- **React Hook Form + Zod** - Forms
- **Lucide React** - Icons
- **qrcode.react** - QR codes
- **MSW** - Mock API for testing

## Design

- Mobile-first (390px breakpoint)
- Dark mode support
- Calm, trustworthy fintech aesthetic
- WCAG AA contrast
- Full keyboard navigation
- Respects `prefers-reduced-motion`

## Disclaimer

This is a technical demonstration. It is not a licensed financial service. See the backend README for the full disclaimer.
