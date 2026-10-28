# SatsBridge web

Single-page app (Vite, React, TypeScript) for senders. It talks to the [backend API](../backend/README.md) and holds no business logic: amounts, fees and rates all come from the server.

```bash
cd frontend
npm install
cp .env.example .env   # VITE_API_URL, default http://localhost:8080/api/v1
npm run dev            # http://localhost:3000 (the backend's default CORS origin)
npm run build          # typechecks, then builds to dist/
```

The backend must be running with `CORS_ALLOWED_ORIGINS=http://localhost:3000` (the `.env.example` default), or `DEV_ROUTES=true`.

## Screens

| Route | What it does |
|---|---|
| `/` | Pick currencies and an amount (what you send, or what they receive). A quote appears automatically with the fee, rate and a countdown. Choose the payout option when a currency has several, enter the recipient, review, confirm. |
| `/pay/:id` | Shows the Lightning invoice as a QR code (with an LNURL alternative), then follows the payment: received, paying out, delivered. Includes the receipt and audit timeline. If the payout fails, it asks for a refund invoice. |
| `/history` | The caller's payments. |

Recipient forms follow the payout option the backend reports: a Nigerian bank account (the account name is looked up and the sender must confirm it), a Lightning Address, an M-Pesa number, or mobile money by country and operator.

## How it works

- **Accounts:** the first API call creates an anonymous user (`POST /users`) and stores its token in `localStorage`. Clearing site data loses the history, and there is no recovery. If the server stops recognising the token, the app silently starts a new account.
- **Rails and limits** come from `GET /currencies`, so a payout option appears only when the backend has a provider for it.
- **Dev button:** in `npm run dev`, a payment on a mock payout rail shows buttons that simulate the provider's callback (needs the backend's `DEV_ROUTES=true`). It is not in production builds.
- **Amounts** are parsed from text into integer minor units without floating point (`src/lib/money.ts`).

## Not built yet

- Admin dashboard (the backend has `/admin/*`).
- Tests (only the build's type check so far).
- Not yet driven in a browser; the API behind it has been run end to end with the local stand-ins. There are no automated UI tests.
