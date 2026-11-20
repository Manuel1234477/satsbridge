# StellarBridge demo script: naira to Kenyan shillings in 3 minutes

Written for: the presenter and one judge watching the screen.

## Before you start

Pick one:

- **Live on Stellar testnet (best):** the backend set up with `stellar_setup` and running with `DEV_ROUTES=true`, and the web app with dev tools on:

  ```bash
  cd backend && cargo run
  cd web && NEXT_PUBLIC_DEV_TOOLS=true npm run dev
  ```

- **No network:** `cd web && NEXT_PUBLIC_MOCK_API=true npm run dev`. Same screens, made-up Stellar hashes, and the transfer "arrives" by itself after 8 seconds.

Open http://localhost:3000 in Chrome, about 390 px wide (phone view) or full width. Start from a fresh profile, or clear `localStorage` for the site.

## 0:00 to 0:20: Frame it

Say: "Ada is in Lagos. Her brother is in Nairobi. She pays in naira from her bank app, and he gets shillings on M-Pesa. In between, the money settles on Stellar, in one atomic transaction anyone can check."

Point at the "Sandbox" badge: no real money moves, and this is not a licensed financial service.

## 0:20 to 0:50: Quote

1. "You send" is **NGN**. Type **150000**.
2. Point at the rate card: `1 NGN ≈ 0.095 KES`, the path **NGN → USD → KES** on the Stellar DEX, the ₦1,500 fee, and what he receives in KES.
3. Say: "That amount is guaranteed. It becomes the minimum output of the Stellar path payment. If the market moves past it, nothing is sent and Ada gets her naira back."
4. Point at the countdown: the rate refreshes when it runs out.

## 0:50 to 1:10: Recipient

1. Click **Continue**. Type the M-Pesa number **0712345678** and click **Use this number**.
2. On the review screen, point out the summary. Click **Confirm and continue**.

## 1:10 to 1:40: Pay by bank transfer

1. The page shows a one-off **account number**, the bank, and the exact amount, with copy buttons.
2. Say: "This is an ordinary Nigerian bank transfer. The account number exists for this payment only, so we know whose money arrived."
3. Click **I have sent the transfer** (in mock mode, wait about 8 seconds instead).

## 1:40 to 2:20: Settled on Stellar, then delivered

1. The tracker moves to **Settled on Stellar**. Scroll to **On Stellar**: "Minted and converted", with the KES delivered and the ledger number.
2. On testnet, click the transaction link. stellar.expert shows two operations: the issuer minting ₦150,000, and the path payment from NGN through USD to KES.
3. Back in the app, use the dev panel's **Success** (the mock M-Pesa provider confirms). The banner reads **Money delivered**, and a second transaction appears: **Tokens burned after payout**.
4. Say: "The KES token only existed while the money was in flight. Once the shillings left our account in Kenya, the token was burned."

## 2:20 to 2:50: When the payout fails

1. Start again with the same amount, and the number **0712340000**. In mock mode this fails by itself; on testnet, use the dev panel's **Failed**.
2. The banner reads **Refund needed**, and **On Stellar** shows **Conversion reversed**.
3. Say: "We undo the conversion on-chain and send the full ₦150,000 back to the account it came from. Ada doesn't have to do anything."
4. The refund completes: **Refunded**.

## 2:50 to 3:00: Close

- Real: Stellar settlement on testnet (accounts, DEX liquidity, path payments, burns, reversals), the quote and refund rules, and the status model.
- Not real here: the Nigerian bank collection, M-Pesa and mobile money. Those are mocks, behind interfaces where real providers (and Stellar anchors) plug in.
- Next: a real NGN collection partner, backed assets through licensed anchors, and more corridors (GHS, XOF, XAF and GNF already work the same way).

## If something goes wrong on stage

- Page stuck on "Loading…": reload once. State is kept on the server (or in `localStorage` in mock mode).
- Quote error on testnet: the DEX needs liquidity. Rerun `cargo run --bin stellar_setup`.
- Want the mock faster: `NEXT_PUBLIC_MOCK_TIME_SCALE=0.3 NEXT_PUBLIC_MOCK_API=true npm run dev`.
