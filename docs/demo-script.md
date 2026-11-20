# StellarBridge demo script: naira to Kenyan shillings in 3 minutes

Written for: the presenter and one judge watching the screen.

## Before you start

Pick one:

This demo needs a collection and a payout provider, and none is wired in yet (see the [implementation status](implementation-status.md)). Until one is, the pay-in and payout steps below cannot be run end to end. The quote, the Stellar settlement and the receipt are real on testnet: set up with `stellar_setup`, then `cd backend && cargo run` and `cd web && npm run dev`.

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
3. Click **I have sent the transfer**.

## 1:40 to 2:20: Settled on Stellar, then delivered

1. The tracker moves to **Settled on Stellar**. Scroll to **On Stellar**: "Minted and converted", with the KES delivered and the ledger number.
2. On testnet, click the transaction link. stellar.expert shows two operations: the issuer minting ₦150,000, and the path payment from NGN through USD to KES.
3. Back in the app, wait for the payout provider to confirm. The banner reads **Money delivered**, and a second transaction appears: **Tokens burned after payout**.
4. Say: "The KES token only existed while the money was in flight. Once the shillings left our account in Kenya, the token was burned."

## 2:20 to 2:50: When the payout fails

1. Start again with the same amount, and the number **0712340000**. The payout provider must report a failure.
2. The banner reads **Refund needed**, and **On Stellar** shows **Conversion reversed**.
3. Say: "We undo the conversion on-chain and send the full ₦150,000 back to the account it came from. Ada doesn't have to do anything."
4. The refund completes: **Refunded**.

## 2:50 to 3:00: Close

- Real: Stellar settlement on testnet (accounts, DEX liquidity, path payments, burns, reversals), the quote and refund rules, and the status model.
- Not real here: the Nigerian bank collection, M-Pesa and mobile money. Those have no provider yet, only interfaces where real providers (and Stellar anchors) plug in.
- Any currency can be the sender's: switch "You send" to KES and the pay-in becomes an M-Pesa Paybill with a reference.
- Next: real collection partners, backed assets through licensed anchors, and more corridors.

## If something goes wrong on stage

- Page stuck on "Loading…": reload once. State is kept on the server.
- Quote error on testnet: the DEX needs liquidity. Rerun `cargo run --bin stellar_setup`.
