//! Stellar network access through Horizon. [`HorizonClient`] is the raw REST client (shared with
//! the `stellar_setup` tool); [`HorizonLedger`] is the settlement adapter built on it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde::Deserialize;

use super::{
    LedgerOp, PathQuote, PreparedTx, StellarAsset, StellarLedger, TxOutcome,
    keys::Keypair,
    pricing::{format_amount, parse_amount},
    tx,
};
use crate::{adapters::AdapterError, domain::Currency};

#[derive(Clone)]
pub struct HorizonClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountRecord {
    #[serde(deserialize_with = "de_i64_str")]
    pub sequence: i64,
    #[serde(default)]
    pub balances: Vec<BalanceRecord>,
}

#[derive(Debug, Deserialize)]
pub struct BalanceRecord {
    pub asset_type: String,
    #[serde(default)]
    pub asset_code: Option<String>,
    #[serde(default)]
    pub asset_issuer: Option<String>,
    pub balance: String,
}

#[derive(Debug, Deserialize)]
pub struct OfferRecord {
    #[serde(deserialize_with = "de_i64_str")]
    pub id: i64,
    pub selling: AssetRecord,
    pub buying: AssetRecord,
}

#[derive(Debug, Deserialize)]
pub struct AssetRecord {
    pub asset_type: String,
    #[serde(default)]
    pub asset_code: Option<String>,
    #[serde(default)]
    pub asset_issuer: Option<String>,
}

impl AssetRecord {
    fn to_asset(&self) -> Option<StellarAsset> {
        (self.asset_type != "native").then(|| StellarAsset {
            code: self.asset_code.clone().unwrap_or_default(),
            issuer: self.asset_issuer.clone().unwrap_or_default(),
        })
    }

    pub fn code(&self) -> &str {
        self.asset_code.as_deref().unwrap_or("XLM")
    }
}

#[derive(Debug, Deserialize)]
pub struct PathRecord {
    pub source_amount: String,
    pub destination_amount: String,
    #[serde(default)]
    pub path: Vec<AssetRecord>,
}

#[derive(Deserialize)]
struct Embedded<T> {
    #[serde(rename = "_embedded")]
    embedded: Records<T>,
}

#[derive(Deserialize)]
struct Records<T> {
    records: Vec<T>,
}

#[derive(Deserialize)]
struct TxRecord {
    #[serde(default)]
    successful: Option<bool>,
    #[serde(default)]
    ledger: Option<i64>,
    #[serde(default)]
    result_xdr: Option<String>,
}

#[derive(Deserialize, Default)]
struct Problem {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    extras: Option<ProblemExtras>,
}

#[derive(Deserialize, Default)]
struct ProblemExtras {
    #[serde(default)]
    result_codes: Option<ResultCodes>,
}

#[derive(Deserialize, Default)]
struct ResultCodes {
    #[serde(default)]
    transaction: Option<String>,
    #[serde(default)]
    operations: Option<Vec<String>>,
}

fn de_i64_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn unavailable(e: impl std::fmt::Display) -> AdapterError {
    AdapterError::Unavailable(format!("Horizon: {e}"))
}

impl HorizonClient {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Option<T>, AdapterError> {
        let res = self
            .http
            .get(format!("{}{path}", self.base_url))
            .query(query)
            .send()
            .await
            .map_err(unavailable)?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = res.status();
        let body = res.bytes().await.map_err(unavailable)?;
        if !status.is_success() {
            let problem: Problem = serde_json::from_slice(&body).unwrap_or_default();
            let msg = format!("{} {}", status.as_u16(), problem.title.unwrap_or_default());
            return Err(if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
                unavailable(msg)
            } else {
                AdapterError::Rejected(format!("Horizon {msg}"))
            });
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| unavailable(format!("unexpected response to {path}: {e}")))
    }

    pub async fn account(&self, account: &str) -> Result<Option<AccountRecord>, AdapterError> {
        self.get(&format!("/accounts/{account}"), &[]).await
    }

    pub async fn offers(&self, account: &str) -> Result<Vec<OfferRecord>, AdapterError> {
        let page: Option<Embedded<OfferRecord>> = self
            .get(&format!("/accounts/{account}/offers"), &[("limit", "200".into())])
            .await?;
        Ok(page.map(|p| p.embedded.records).unwrap_or_default())
    }

    pub async fn strict_send_paths(
        &self,
        source: &StellarAsset,
        amount: i64,
        destination: &StellarAsset,
    ) -> Result<Vec<PathRecord>, AdapterError> {
        let page: Option<Embedded<PathRecord>> = self
            .get(
                "/paths/strict-send",
                &[
                    ("source_asset_type", asset_type(source).into()),
                    ("source_asset_code", source.code.clone()),
                    ("source_asset_issuer", source.issuer.clone()),
                    ("source_amount", format_amount(amount)),
                    ("destination_assets", destination.canonical()),
                ],
            )
            .await?;
        Ok(page.map(|p| p.embedded.records).unwrap_or_default())
    }

    pub async fn strict_receive_paths(
        &self,
        source: &StellarAsset,
        destination: &StellarAsset,
        amount: i64,
    ) -> Result<Vec<PathRecord>, AdapterError> {
        let page: Option<Embedded<PathRecord>> = self
            .get(
                "/paths/strict-receive",
                &[
                    ("source_assets", source.canonical()),
                    ("destination_asset_type", asset_type(destination).into()),
                    ("destination_asset_code", destination.code.clone()),
                    ("destination_asset_issuer", destination.issuer.clone()),
                    ("destination_amount", format_amount(amount)),
                ],
            )
            .await?;
        Ok(page.map(|p| p.embedded.records).unwrap_or_default())
    }

    /// Submits a signed envelope and waits for it to be applied. Resubmitting an envelope that
    /// is already on the ledger returns its original result, so this is safe to repeat.
    pub async fn submit(&self, envelope_xdr: &str) -> Result<TxOutcome, AdapterError> {
        let res = self
            .http
            .post(format!("{}/transactions", self.base_url))
            .form(&[("tx", envelope_xdr)])
            .send()
            .await
            .map_err(unavailable)?;
        let status = res.status();
        let body = res.bytes().await.map_err(unavailable)?;
        if status.is_success() {
            let record: TxRecord = serde_json::from_slice(&body).map_err(unavailable)?;
            return Ok(outcome(record));
        }
        if status == StatusCode::BAD_REQUEST {
            let problem: Problem = serde_json::from_slice(&body).unwrap_or_default();
            let codes = problem.extras.and_then(|e| e.result_codes).unwrap_or_default();
            let mut reason = codes.transaction.unwrap_or_else(|| "rejected".into());
            if let Some(ops) = codes.operations {
                reason = format!("{reason}: {}", ops.join(", "));
            }
            // Rejected at submission: never applied, and this envelope is never resubmitted.
            return Ok(TxOutcome::Failed { reason });
        }
        // 504 means Horizon gave up waiting; the transaction may still be applied.
        Err(unavailable(format!("submission returned {}", status.as_u16())))
    }

    pub async fn transaction(&self, hash: &str) -> Result<TxOutcome, AdapterError> {
        let record: Option<TxRecord> = self.get(&format!("/transactions/{hash}"), &[]).await?;
        Ok(record.map(outcome).unwrap_or(TxOutcome::NotFound))
    }

    /// Funds a new testnet account with friendbot.
    pub async fn friendbot(&self, friendbot_url: &str, account: &str) -> Result<(), AdapterError> {
        let res = self
            .http
            .get(friendbot_url)
            .query(&[("addr", account)])
            .send()
            .await
            .map_err(unavailable)?;
        if !res.status().is_success() {
            return Err(AdapterError::Rejected(format!(
                "friendbot returned {} for {account}",
                res.status().as_u16()
            )));
        }
        Ok(())
    }
}

fn outcome(record: TxRecord) -> TxOutcome {
    let ledger = record.ledger.unwrap_or_default();
    match (record.result_xdr, record.successful) {
        (Some(xdr), _) => tx::decode_result(&xdr, ledger),
        (None, Some(true)) => TxOutcome::Succeeded {
            ledger,
            delivered: None,
        },
        (None, _) => TxOutcome::Failed {
            reason: "transaction failed".into(),
        },
    }
}

fn asset_type(a: &StellarAsset) -> &'static str {
    if a.code.len() <= 4 {
        "credit_alphanum4"
    } else {
        "credit_alphanum12"
    }
}

fn hops(source: &StellarAsset, path: &[AssetRecord], destination: &StellarAsset) -> Vec<String> {
    std::iter::once(source.code.clone())
        .chain(path.iter().map(|a| a.code().to_string()))
        .chain(std::iter::once(destination.code.clone()))
        .collect()
}

#[derive(Clone)]
pub struct HorizonConfig {
    pub horizon_url: String,
    pub network_passphrase: String,
    pub issuer_secret: String,
    pub treasury_secret: String,
    pub bridge_code: String,
    /// How long a signed transaction stays valid.
    pub tx_timeout_secs: i64,
    /// Explorer base for links, e.g. `https://stellar.expert/explorer/testnet`.
    pub explorer_url: Option<String>,
}

pub struct HorizonLedger {
    client: HorizonClient,
    config: HorizonConfig,
    issuer: Keypair,
    treasury: Keypair,
    assets: BTreeMap<Currency, StellarAsset>,
}

impl HorizonLedger {
    pub fn new(http: reqwest::Client, config: HorizonConfig) -> anyhow::Result<Self> {
        let issuer =
            Keypair::from_secret(&config.issuer_secret).map_err(|e| anyhow::anyhow!("STELLAR_ISSUER_SECRET: {e}"))?;
        let treasury = Keypair::from_secret(&config.treasury_secret)
            .map_err(|e| anyhow::anyhow!("STELLAR_TREASURY_SECRET: {e}"))?;
        let assets = Currency::ALL
            .into_iter()
            .map(|c| {
                (
                    c,
                    StellarAsset {
                        code: c.code().into(),
                        issuer: issuer.public_key(),
                    },
                )
            })
            .collect();
        Ok(Self {
            client: HorizonClient::new(http, &config.horizon_url),
            config,
            issuer,
            treasury,
            assets,
        })
    }

    pub fn client(&self) -> &HorizonClient {
        &self.client
    }

    fn asset_for(&self, currency: Currency) -> Result<&StellarAsset, AdapterError> {
        self.assets
            .get(&currency)
            .ok_or_else(|| AdapterError::Rejected(format!("{currency} has no Stellar asset")))
    }

    /// Checks that both accounts exist and the treasury trusts every token. Run at startup.
    pub async fn verify(&self) -> anyhow::Result<()> {
        let issuer = self.issuer.public_key();
        self.client
            .account(&issuer)
            .await?
            .ok_or_else(|| anyhow::anyhow!("issuer {issuer} does not exist on this network; run stellar_setup"))?;
        let treasury = self.treasury.public_key();
        let record =
            self.client.account(&treasury).await?.ok_or_else(|| {
                anyhow::anyhow!("treasury {treasury} does not exist on this network; run stellar_setup")
            })?;
        // Intermediate path assets (the bridge) need no trustline; only what the treasury holds.
        for asset in self.assets.values() {
            let trusted = record.balances.iter().any(|b| {
                b.asset_code.as_deref() == Some(asset.code.as_str())
                    && b.asset_issuer.as_deref() == Some(asset.issuer.as_str())
            });
            if !trusted {
                anyhow::bail!("treasury has no trustline for {}; run stellar_setup", asset.code);
            }
        }
        Ok(())
    }

    /// Best strict-send path delivering at least `dest_min`, as intermediate assets.
    async fn send_path(
        &self,
        source: &StellarAsset,
        amount: i64,
        destination: &StellarAsset,
    ) -> Result<(Vec<StellarAsset>, i64), AdapterError> {
        let best = self
            .client
            .strict_send_paths(source, amount, destination)
            .await?
            .into_iter()
            .filter_map(|r| Some((parse_amount(&r.destination_amount)?, r.path)))
            .max_by_key(|(amount, _)| *amount)
            .ok_or_else(|| {
                AdapterError::Unavailable(format!("no DEX path from {} to {}", source.code, destination.code))
            })?;
        let path = best.1.iter().filter_map(AssetRecord::to_asset).collect();
        Ok((path, best.0))
    }
}

#[async_trait]
impl StellarLedger for HorizonLedger {
    fn name(&self) -> &'static str {
        "stellar-testnet"
    }

    fn network_passphrase(&self) -> &str {
        &self.config.network_passphrase
    }

    fn asset(&self, currency: Currency) -> Option<StellarAsset> {
        self.assets.get(&currency).cloned()
    }

    fn bridge_asset(&self) -> StellarAsset {
        StellarAsset {
            code: self.config.bridge_code.clone(),
            issuer: self.issuer.public_key(),
        }
    }

    fn treasury(&self) -> String {
        self.treasury.public_key()
    }

    fn explorer_url(&self, hash: &str) -> Option<String> {
        self.config
            .explorer_url
            .as_ref()
            .map(|base| format!("{}/tx/{hash}", base.trim_end_matches('/')))
    }

    async fn quote_send(
        &self,
        source: Currency,
        amount: i64,
        destination: Currency,
    ) -> Result<PathQuote, AdapterError> {
        let (src, dst) = (self.asset_for(source)?, self.asset_for(destination)?);
        let best = self
            .client
            .strict_send_paths(src, amount, dst)
            .await?
            .into_iter()
            .filter_map(|r| Some((parse_amount(&r.destination_amount)?, r)))
            .max_by_key(|(amount, _)| *amount)
            .ok_or_else(|| AdapterError::Unavailable(format!("no DEX liquidity from {source} to {destination}")))?;
        Ok(PathQuote {
            source_amount: amount,
            destination_amount: best.0,
            hops: hops(src, &best.1.path, dst),
        })
    }

    async fn quote_receive(
        &self,
        source: Currency,
        destination: Currency,
        amount: i64,
    ) -> Result<PathQuote, AdapterError> {
        let (src, dst) = (self.asset_for(source)?, self.asset_for(destination)?);
        let best = self
            .client
            .strict_receive_paths(src, dst, amount)
            .await?
            .into_iter()
            .filter_map(|r| Some((parse_amount(&r.source_amount)?, r)))
            .min_by_key(|(amount, _)| *amount)
            .ok_or_else(|| AdapterError::Unavailable(format!("no DEX liquidity from {source} to {destination}")))?;
        Ok(PathQuote {
            source_amount: best.0,
            destination_amount: amount,
            hops: hops(src, &best.1.path, dst),
        })
    }

    async fn prepare(&self, op: &LedgerOp, memo: &str) -> Result<PreparedTx, AdapterError> {
        let issuer = self.issuer.public_key();
        let treasury = self.treasury.public_key();
        let internal = |e: anyhow::Error| AdapterError::Rejected(format!("cannot build transaction: {e}"));

        let (operations, signers): (Vec<_>, Vec<&Keypair>) = match op {
            LedgerOp::Settle {
                source,
                destination,
                mint,
                send,
                dest_min,
            } => {
                let (src, dst) = (self.asset_for(*source)?, self.asset_for(*destination)?);
                let (path, best) = self.send_path(src, *send, dst).await?;
                if best < *dest_min {
                    return Err(AdapterError::Rejected(format!(
                        "the DEX now delivers {} {}, below the quoted {}",
                        format_amount(best),
                        dst.code,
                        format_amount(*dest_min)
                    )));
                }
                let ops = vec![
                    tx::payment(Some(&issuer), &treasury, src, *mint).map_err(internal)?,
                    tx::path_payment_strict_send(src, *send, &treasury, dst, *dest_min, &path).map_err(internal)?,
                ];
                (ops, vec![&self.treasury, &self.issuer])
            }
            LedgerOp::Redeem { currency, amount } => {
                let a = self.asset_for(*currency)?;
                (
                    vec![tx::payment(None, &issuer, a, *amount).map_err(internal)?],
                    vec![&self.treasury],
                )
            }
            LedgerOp::Reverse { from, to, amount, burn } => {
                let (src, dst) = (self.asset_for(*from)?, self.asset_for(*to)?);
                let (path, _) = self.send_path(src, *amount, dst).await?;
                let mut ops =
                    vec![tx::path_payment_strict_send(src, *amount, &issuer, dst, 1, &path).map_err(internal)?];
                if *burn > 0 {
                    ops.push(tx::payment(None, &issuer, dst, *burn).map_err(internal)?);
                }
                (ops, vec![&self.treasury])
            }
        };

        let account = self
            .client
            .account(&treasury)
            .await?
            .ok_or_else(|| AdapterError::Unavailable("treasury account not found".into()))?;
        let valid_until = Utc::now() + Duration::seconds(self.config.tx_timeout_secs);
        let built = tx::build_and_sign(
            &self.config.network_passphrase,
            &treasury,
            account.sequence + 1,
            memo,
            valid_until.timestamp().max(0) as u64,
            operations,
            &signers,
        )
        .map_err(internal)?;
        Ok(PreparedTx {
            hash: built.hash,
            envelope_xdr: built.envelope_xdr,
            valid_until,
        })
    }

    async fn submit(&self, prepared: &PreparedTx) -> Result<TxOutcome, AdapterError> {
        self.client.submit(&prepared.envelope_xdr).await
    }

    async fn lookup(&self, hash: &str) -> Result<TxOutcome, AdapterError> {
        self.client.transaction(hash).await
    }
}
