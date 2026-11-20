//! Sets up (or refreshes) everything SatsBridge needs on Stellar testnet:
//!
//! 1. Three accounts, funded by friendbot: the **issuer** (issues `NGN`, `KES`, `GHS`, `XOF`,
//!    `XAF`, `GNF` and the `USD` bridge token), the **treasury** (holds tokens while a payment
//!    is in flight) and a **market maker** (provides DEX liquidity).
//! 2. Trustlines from the treasury and market maker to every token.
//! 3. Market maker inventory, minted by the issuer (topped up to the target on each run).
//! 4. Two-sided offers between every currency and `USD`, at the reference FX rate plus a spread,
//!    so path payments like NGN -> USD -> KES always find liquidity.
//!
//! Safe to rerun: existing accounts and trustlines are kept and offers are re-priced in place.
//! Testnet is reset every few months; rerun this afterwards.
//!
//! ```text
//! cargo run --bin stellar_setup
//! ```
//!
//! Reads `STELLAR_ISSUER_SECRET`, `STELLAR_TREASURY_SECRET` and `STELLAR_MARKET_MAKER_SECRET`
//! from the environment or `.env`; missing ones are generated and printed. Also reads
//! `STELLAR_HORIZON_URL`, `STELLAR_FRIENDBOT_URL`, `MM_SPREAD_BPS` (default 30),
//! `MM_DEPTH_USD` (offer size per side, default 250000) and the `FX_*` settings.

use std::{env, time::Duration};

use anyhow::{Context, bail};
use chrono::Utc;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use satsbridge::{
    adapters::stellar::{
        STROOPS_PER_UNIT, StellarAsset, TESTNET_FRIENDBOT, TESTNET_HORIZON, TESTNET_PASSPHRASE, TxOutcome,
        horizon::HorizonClient,
        keys::Keypair,
        pricing::{best_rational, parse_amount},
        tx,
    },
    config::FxConfig,
    domain::Currency,
    state::{build_fx, http_client},
};

struct Setup {
    horizon: HorizonClient,
    friendbot: String,
}

impl Setup {
    async fn ensure_account(&self, name: &str, kp: &Keypair) -> anyhow::Result<()> {
        let id = kp.public_key();
        if self.horizon.account(&id).await?.is_some() {
            println!("  {name}: {id} (exists)");
            return Ok(());
        }
        self.horizon.friendbot(&self.friendbot, &id).await?;
        for _ in 0..20 {
            if self.horizon.account(&id).await?.is_some() {
                println!("  {name}: {id} (funded by friendbot)");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        bail!("{name} account {id} did not appear after friendbot funding")
    }

    /// Builds, signs and submits one transaction from `source`.
    async fn submit(
        &self,
        what: &str,
        source: &Keypair,
        ops: Vec<stellar_xdr::Operation>,
        signers: &[&Keypair],
    ) -> anyhow::Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        for chunk_start in (0..ops.len()).step_by(100) {
            let chunk = ops[chunk_start..(chunk_start + 100).min(ops.len())].to_vec();
            let account = self
                .horizon
                .account(&source.public_key())
                .await?
                .context("source account missing")?;
            let built = tx::build_and_sign(
                TESTNET_PASSPHRASE,
                &source.public_key(),
                account.sequence + 1,
                "satsbridge setup",
                (Utc::now().timestamp() + 120) as u64,
                chunk,
                signers,
            )?;
            match self.horizon.submit(&built.envelope_xdr).await? {
                TxOutcome::Succeeded { ledger, .. } => println!("  {what}: ok (ledger {ledger}, tx {})", built.hash),
                TxOutcome::Failed { reason } => bail!("{what} failed: {reason}"),
                TxOutcome::NotFound => bail!("{what}: no result"),
            }
        }
        Ok(())
    }
}

fn keypair(var: &str, generated: &mut Vec<(String, String)>) -> anyhow::Result<Keypair> {
    match env::var(var).ok().filter(|v| !v.trim().is_empty()) {
        Some(secret) => Keypair::from_secret(&secret).with_context(|| var.to_string()),
        None => {
            let kp = Keypair::random()?;
            generated.push((var.to_string(), kp.secret()));
            Ok(kp)
        }
    }
}

fn units(stroops: i64) -> Decimal {
    Decimal::new(stroops, 7)
}

fn stroops(units: Decimal) -> anyhow::Result<i64> {
    (units * Decimal::from(STROOPS_PER_UNIT))
        .floor()
        .to_i64()
        .context("amount out of range")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let horizon_url = env::var("STELLAR_HORIZON_URL").unwrap_or_else(|_| TESTNET_HORIZON.into());
    let setup = Setup {
        horizon: HorizonClient::new(http_client(Duration::from_secs(60))?, &horizon_url),
        friendbot: env::var("STELLAR_FRIENDBOT_URL").unwrap_or_else(|_| TESTNET_FRIENDBOT.into()),
    };
    let spread_bps: i64 = env::var("MM_SPREAD_BPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let depth_usd: Decimal = env::var("MM_DEPTH_USD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(Decimal::from(250_000));
    let bridge_code = env::var("STELLAR_BRIDGE_ASSET").unwrap_or_else(|_| "USD".into());

    let mut generated = Vec::new();
    let issuer = keypair("STELLAR_ISSUER_SECRET", &mut generated)?;
    let treasury = keypair("STELLAR_TREASURY_SECRET", &mut generated)?;
    let mm = keypair("STELLAR_MARKET_MAKER_SECRET", &mut generated)?;

    println!("Stellar testnet setup ({horizon_url})\n\n1. Accounts");
    setup.ensure_account("issuer", &issuer).await?;
    setup.ensure_account("treasury", &treasury).await?;
    setup.ensure_account("market maker", &mm).await?;

    let asset = |code: &str| StellarAsset {
        code: code.into(),
        issuer: issuer.public_key(),
    };
    let bridge = asset(&bridge_code);
    let fiat: Vec<(Currency, StellarAsset)> = Currency::ALL.into_iter().map(|c| (c, asset(c.code()))).collect();

    println!("\n2. Trustlines");
    for (name, kp) in [("treasury", &treasury), ("market maker", &mm)] {
        let record = setup
            .horizon
            .account(&kp.public_key())
            .await?
            .context("account missing")?;
        let has = |a: &StellarAsset| {
            record
                .balances
                .iter()
                .any(|b| b.asset_code.as_deref() == Some(&a.code) && b.asset_issuer.as_deref() == Some(&a.issuer))
        };
        let ops = fiat
            .iter()
            .map(|(_, a)| a)
            .chain(std::iter::once(&bridge))
            .filter(|a| !has(a))
            .map(|a| tx::change_trust(None, a))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if ops.is_empty() {
            println!("  {name}: all present");
        }
        setup.submit(&format!("{name} trustlines"), kp, ops, &[kp]).await?;
    }

    println!("\n3. Reference rates (units per USD)");
    let fx = build_fx(&FxConfig::from_env()?)?;
    let mut rates = Vec::new();
    for (currency, a) in &fiat {
        let rate = fx
            .usd_rate(*currency)
            .await
            .with_context(|| format!("no USD rate for {currency}"))?;
        println!("  {currency}: {} ({})", rate.rate, rate.source);
        rates.push((*currency, a.clone(), rate.rate));
    }

    println!("\n4. Market maker inventory");
    let record = setup
        .horizon
        .account(&mm.public_key())
        .await?
        .context("market maker missing")?;
    let balance = |a: &StellarAsset| {
        record
            .balances
            .iter()
            .find(|b| b.asset_code.as_deref() == Some(&a.code) && b.asset_issuer.as_deref() == Some(&a.issuer))
            .and_then(|b| parse_amount(&b.balance))
            .unwrap_or(0)
    };
    // Twice the offer depth of each currency, and of USD for every pair.
    let mut targets: Vec<(StellarAsset, i64)> = rates
        .iter()
        .map(|(_, a, rate)| Ok((a.clone(), stroops(depth_usd * Decimal::from(2) * rate)?)))
        .collect::<anyhow::Result<_>>()?;
    targets.push((
        bridge.clone(),
        stroops(depth_usd * Decimal::from(2 * rates.len() as i64))?,
    ));
    let mut mint_ops = Vec::new();
    for (a, target) in &targets {
        let missing = target - balance(a);
        if missing > 0 {
            println!("  mint {} {}", units(missing).normalize(), a.code);
            mint_ops.push(tx::payment(Some(&issuer.public_key()), &mm.public_key(), a, missing)?);
        }
    }
    if mint_ops.is_empty() {
        println!("  inventory is full");
    }
    setup.submit("inventory", &mm, mint_ops, &[&mm, &issuer]).await?;

    println!("\n5. Offers (spread {spread_bps} bps per side)");
    let existing = setup.horizon.offers(&mm.public_key()).await?;
    let offer_id = |selling: &str, buying: &str| {
        existing
            .iter()
            .find(|o| o.selling.code() == selling && o.buying.code() == buying)
            .map(|o| o.id)
            .unwrap_or(0)
    };
    let markup = Decimal::from(10_000 + spread_bps) / Decimal::from(10_000);
    let mut offer_ops = Vec::new();
    for (currency, a, rate) in &rates {
        // Sell the currency for USD: USD per unit, marked up.
        let sell_fiat = best_rational(Decimal::ONE / rate * markup).context("price out of range")?;
        offer_ops.push(tx::manage_sell_offer(
            a,
            &bridge,
            stroops(depth_usd * rate)?,
            sell_fiat,
            offer_id(&a.code, &bridge.code),
        )?);
        // Sell USD for the currency: units per USD, marked up.
        let sell_usd = best_rational(rate * markup).context("price out of range")?;
        offer_ops.push(tx::manage_sell_offer(
            &bridge,
            a,
            stroops(depth_usd)?,
            sell_usd,
            offer_id(&bridge.code, &a.code),
        )?);
        println!(
            "  {currency}/USD: sells {currency} at {:.8} USD, sells USD at {:.4} {currency}",
            Decimal::from(sell_fiat.0) / Decimal::from(sell_fiat.1),
            Decimal::from(sell_usd.0) / Decimal::from(sell_usd.1)
        );
    }
    setup.submit("offers", &mm, offer_ops, &[&mm]).await?;

    println!("\nDone. Issuer {} issues the tokens; see them at", issuer.public_key());
    println!(
        "  https://stellar.expert/explorer/testnet/account/{}",
        issuer.public_key()
    );
    if generated.is_empty() {
        println!("\nAll keys came from the environment; nothing to add to .env.");
    } else {
        println!("\nNew testnet keys. Add these to backend/.env (testnet only, never reuse on mainnet):\n");
        for (var, secret) in &generated {
            println!("{var}={secret}");
        }
    }
    Ok(())
}
