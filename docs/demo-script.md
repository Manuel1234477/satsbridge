# SatsBridge demo script: XOF to NGN in 3 minutes

Written for: the presenter and one judge watching the screen.

## Before you start

- Run the app in mock mode, so the demo works with no backend, no LNbits and no network:

  ```bash
  cd frontend
  NEXT_PUBLIC_MOCK_API=true npm run dev
  ```

  Open http://localhost:3000 in Chrome, window about 390 px wide (phone view) or full width.
- Start from a fresh browser profile or clear `localStorage` for `localhost:3000`, so the first quote and sign-up are clean.

Mock timings at normal speed: the sender's payment is detected 8 s after the invoice is shown, the payout starts at 11 s, and the outcome lands at 15 s. A failed payout needs refunding at about 18 s.

## 0:00 to 0:20: Frame it

Say: "SatsBridge moves money between African currencies. The sender sees their own currency, and the recipient sees theirs. Bitcoin's Lightning Network settles the middle, and you only see it on the pay screen."

Point at the amber "Demo mode" bar at the bottom: no real money moves, and this is not a licensed financial service.

## 0:20 to 0:50: Quote

1. Leave "You send" as **XOF**. Type **10000** in "Amount you send".
2. Point at the quote card: the rate in plain currencies (`1 XOF ≈ 2.5 NGN`), the fee in XOF, and what the recipient receives in **NGN**.
3. Point at the countdown: "Quote expires in …s". Say the rate refreshes on its own when it runs out, and the sender can never pay on an expired rate.

Optional, if time allows: type 777 instead of 10000 to show a 5-second quote that expires. Let it refresh once.

## 0:50 to 1:20: Recipient

1. Click **Continue**.
2. In the bank list, choose **GTBank**. Type account **0123456789**.
3. Click **Check account**. The "Confirm account holder" card shows the name the bank returned.
4. Say: "The sender must confirm the name before anything is paid. This stops payments to the wrong account." Click **Yes, this is the account holder**.
5. On the review screen, point out the summary and the masked account (`GTBank · ••••••6789`). Click **Confirm and continue**.

## 1:20 to 1:45: Pay

1. The page shows a Lightning QR code and the amount in sats, with a countdown for the invoice.
2. Say: "This is the only place the sender sees Bitcoin. Scan it with any Lightning wallet, or use Open in wallet or Copy invoice."
3. Wait. About 8 seconds in, the status moves to **Payment received**.

## 1:45 to 2:15: Track and receipt

1. Walk through the progress steps: received, converting, paying recipient, delivered. Say: "A bank can take a while. A long 'paying recipient' step is normal, and we say so on screen."
2. At about 15 seconds the banner reads **Money delivered** and the receipt appears: amounts, rate, fee, recipient name, masked account, provider reference.
3. Open **Details** and point at the audit trail and raw status. Say: "This is the same data the backend keeps for audit."

## 2:15 to 2:50: The payout fails, and the refund path

1. Click **Send money**, or go to `/`, and repeat the flow. Use the same amount, bank and account **0000000000** (any account ending in 0000 fails).
2. About 18 seconds after the invoice, the banner reads **Refund needed**: "The bank rejected the transfer. Add a Lightning invoice for the same sats to get your money back."
3. Paste a wrong amount: `lnbc<N×10+1>n1mock`, where N is the sats shown on screen (for 25,539 sats, paste `lnbc255391n1mock`). Show the server's message: "the refund invoice must be for exactly N sats". Say: "The backend checks the amount, so a wrong invoice cannot lose money."
4. Paste the correct invoice. The screen shows **Refunded** with the time and the sats refunded.

   Note for the presenter: the mock reads the BOLT11 amount, where `n` is 100 msat. For 25,539 sats, the exact invoice is `lnbc255390n1mock`.

## 2:50 to 3:00: Close

Say what is real and what is not:

- Real: the quote logic, the recipient check, the invoice, the refund rules and the status model are written against the real backend API.
- Not real in this demo: money movement, the bank, and the Lightning node. Mock mode stands in for them. The real-backend path has not been tested end to end.
- Next: English and French, an accessibility audit, and the real backend with LNbits.

## If something goes wrong on stage

- Page stuck on "Loading…": reload once. The mock state is in `localStorage`, so the flow survives a reload.
- A payment you want to repeat: use a different amount, or clear `localStorage` for the site.
- Want it faster: restart with `NEXT_PUBLIC_MOCK_TIME_SCALE=0.3 NEXT_PUBLIC_MOCK_API=true npm run dev`.
