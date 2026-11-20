//! Transaction building, signing and result decoding with `stellar-xdr`.

use anyhow::{Context, anyhow};
use sha2::{Digest, Sha256};
use stellar_xdr::{
    AccountId, AlphaNum4, AlphaNum12, Asset, AssetCode4, AssetCode12, ChangeTrustAsset, ChangeTrustOp,
    DecoratedSignature, Limits, ManageSellOfferOp, Memo, MuxedAccount, Operation, OperationBody, OperationResult,
    OperationResultTr, PathPaymentStrictSendOp, PathPaymentStrictSendResult, PaymentOp, Preconditions, Price,
    PublicKey, ReadXdr, SequenceNumber, Signature, SignatureHint, TimeBounds, TimePoint, Transaction,
    TransactionEnvelope, TransactionExt, TransactionResult, TransactionResultResult, TransactionV1Envelope, Uint256,
    WriteXdr,
};

use super::{StellarAsset, TxOutcome, keys::Keypair, keys::decode_account};

/// Base fee per operation, in stroops. Testnet's minimum is 100; this leaves room for surge pricing.
pub const BASE_FEE: u32 = 1_000;

pub fn network_id(passphrase: &str) -> [u8; 32] {
    Sha256::digest(passphrase.as_bytes()).into()
}

fn account_id(account: &str) -> anyhow::Result<AccountId> {
    Ok(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(decode_account(
        account,
    )?))))
}

fn muxed(account: &str) -> anyhow::Result<MuxedAccount> {
    Ok(MuxedAccount::Ed25519(Uint256(decode_account(account)?)))
}

pub fn asset(a: &StellarAsset) -> anyhow::Result<Asset> {
    let code = a.code.as_bytes();
    let issuer = account_id(&a.issuer)?;
    match code.len() {
        1..=4 => {
            let mut buf = [0u8; 4];
            buf[..code.len()].copy_from_slice(code);
            Ok(Asset::CreditAlphanum4(AlphaNum4 {
                asset_code: AssetCode4(buf),
                issuer,
            }))
        }
        5..=12 => {
            let mut buf = [0u8; 12];
            buf[..code.len()].copy_from_slice(code);
            Ok(Asset::CreditAlphanum12(AlphaNum12 {
                asset_code: AssetCode12(buf),
                issuer,
            }))
        }
        _ => Err(anyhow!("invalid asset code {:?}", a.code)),
    }
}

pub fn payment(source: Option<&str>, destination: &str, a: &StellarAsset, amount: i64) -> anyhow::Result<Operation> {
    Ok(Operation {
        source_account: source.map(muxed).transpose()?,
        body: OperationBody::Payment(PaymentOp {
            destination: muxed(destination)?,
            asset: asset(a)?,
            amount,
        }),
    })
}

pub fn path_payment_strict_send(
    send_asset: &StellarAsset,
    send_amount: i64,
    destination: &str,
    dest_asset: &StellarAsset,
    dest_min: i64,
    path: &[StellarAsset],
) -> anyhow::Result<Operation> {
    let path = path.iter().map(asset).collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Operation {
        source_account: None,
        body: OperationBody::PathPaymentStrictSend(PathPaymentStrictSendOp {
            send_asset: asset(send_asset)?,
            send_amount,
            destination: muxed(destination)?,
            dest_asset: asset(dest_asset)?,
            dest_min,
            path: path.try_into().map_err(|_| anyhow!("path is longer than 5 assets"))?,
        }),
    })
}

/// Creates (`offer_id == 0`), updates, or deletes (`amount == 0`) a sell offer.
/// `price` is units of `buying` per unit of `selling`.
pub fn manage_sell_offer(
    selling: &StellarAsset,
    buying: &StellarAsset,
    amount: i64,
    price: (i32, i32),
    offer_id: i64,
) -> anyhow::Result<Operation> {
    Ok(Operation {
        source_account: None,
        body: OperationBody::ManageSellOffer(ManageSellOfferOp {
            selling: asset(selling)?,
            buying: asset(buying)?,
            amount,
            price: Price { n: price.0, d: price.1 },
            offer_id,
        }),
    })
}

pub fn change_trust(source: Option<&str>, a: &StellarAsset) -> anyhow::Result<Operation> {
    let line = match asset(a)? {
        Asset::CreditAlphanum4(x) => ChangeTrustAsset::CreditAlphanum4(x),
        Asset::CreditAlphanum12(x) => ChangeTrustAsset::CreditAlphanum12(x),
        Asset::Native => unreachable!("credit assets only"),
    };
    Ok(Operation {
        source_account: source.map(muxed).transpose()?,
        body: OperationBody::ChangeTrust(ChangeTrustOp { line, limit: i64::MAX }),
    })
}

pub struct Built {
    pub hash: String,
    pub envelope_xdr: String,
}

/// Builds a transaction from `source` at sequence `seq`, valid until `max_time` (unix seconds),
/// signed by every keypair in `signers`.
pub fn build_and_sign(
    network_passphrase: &str,
    source: &str,
    seq: i64,
    memo: &str,
    max_time: u64,
    operations: Vec<Operation>,
    signers: &[&Keypair],
) -> anyhow::Result<Built> {
    let fee = BASE_FEE
        .checked_mul(operations.len() as u32)
        .context("too many operations")?;
    let memo = if memo.is_empty() {
        Memo::None
    } else {
        let text: String = memo.chars().take(28).collect();
        Memo::Text(
            text.as_bytes()
                .to_vec()
                .try_into()
                .map_err(|_| anyhow!("memo too long"))?,
        )
    };
    let tx = Transaction {
        source_account: muxed(source)?,
        fee,
        seq_num: SequenceNumber(seq),
        cond: Preconditions::Time(TimeBounds {
            min_time: TimePoint(0),
            max_time: TimePoint(max_time),
        }),
        memo,
        operations: operations.try_into().map_err(|_| anyhow!("too many operations"))?,
        ext: TransactionExt::V0,
    };
    let mut envelope = TransactionV1Envelope {
        tx,
        signatures: Default::default(),
    };
    let hash = envelope.hash(network_id(network_passphrase))?;
    let signatures = signers
        .iter()
        .map(|kp| {
            Ok(DecoratedSignature {
                hint: SignatureHint(kp.hint()),
                signature: Signature(
                    kp.sign(&hash)
                        .to_vec()
                        .try_into()
                        .map_err(|_| anyhow!("bad signature"))?,
                ),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    envelope.signatures = signatures.try_into().map_err(|_| anyhow!("too many signatures"))?;
    Ok(Built {
        hash: hex::encode(hash),
        envelope_xdr: TransactionEnvelope::Tx(envelope).to_xdr_base64(Limits::none())?,
    })
}

/// The path payment's delivered amount, from a successful transaction's operation results.
fn delivered(results: &[OperationResult]) -> Option<i64> {
    results.iter().find_map(|r| match r {
        OperationResult::OpInner(OperationResultTr::PathPaymentStrictSend(PathPaymentStrictSendResult::Success(s))) => {
            Some(s.last.amount)
        }
        _ => None,
    })
}

/// Decodes a `TransactionResult` (Horizon's `result_xdr`) into an outcome.
pub fn decode_result(result_xdr: &str, ledger: i64) -> TxOutcome {
    let result = match TransactionResult::from_xdr_base64(result_xdr, Limits::none()) {
        Ok(r) => r,
        Err(e) => {
            return TxOutcome::Failed {
                reason: format!("undecodable transaction result: {e}"),
            };
        }
    };
    match result.result {
        TransactionResultResult::TxSuccess(ops) => TxOutcome::Succeeded {
            ledger,
            delivered: delivered(&ops),
        },
        TransactionResultResult::TxFailed(ops) => TxOutcome::Failed {
            reason: format!("tx_failed: {}", op_codes(&ops).join(", ")),
        },
        other => TxOutcome::Failed {
            reason: other.name().to_string(),
        },
    }
}

fn op_codes(ops: &[OperationResult]) -> Vec<String> {
    ops.iter()
        .map(|r| match r {
            OperationResult::OpInner(OperationResultTr::PathPaymentStrictSend(p)) => {
                format!("path_payment_strict_send:{}", p.name())
            }
            OperationResult::OpInner(OperationResultTr::Payment(p)) => format!("payment:{}", p.name()),
            OperationResult::OpInner(inner) => inner.name().to_string(),
            other => other.name().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use stellar_xdr::{PathPaymentStrictSendResultSuccess, SimplePaymentResult, TransactionResultExt};

    use super::*;

    fn ngn(issuer: &str) -> StellarAsset {
        StellarAsset {
            code: "NGN".into(),
            issuer: issuer.into(),
        }
    }

    #[test]
    fn signs_a_verifiable_envelope() {
        let issuer = Keypair::random().unwrap();
        let treasury = Keypair::random().unwrap();
        let ops = vec![
            payment(
                Some(&issuer.public_key()),
                &treasury.public_key(),
                &ngn(&issuer.public_key()),
                10,
            )
            .unwrap(),
            path_payment_strict_send(
                &ngn(&issuer.public_key()),
                10,
                &treasury.public_key(),
                &StellarAsset {
                    code: "KES".into(),
                    issuer: issuer.public_key(),
                },
                1,
                &[StellarAsset {
                    code: "USD".into(),
                    issuer: issuer.public_key(),
                }],
            )
            .unwrap(),
        ];
        let built = build_and_sign(
            super::super::TESTNET_PASSPHRASE,
            &treasury.public_key(),
            42,
            "pay_0123456789abcdef0123456789abcdef",
            1_900_000_000,
            ops,
            &[&treasury, &issuer],
        )
        .unwrap();
        let env = TransactionEnvelope::from_xdr_base64(&built.envelope_xdr, Limits::none()).unwrap();
        let TransactionEnvelope::Tx(v1) = &env else {
            panic!("v1 envelope")
        };
        assert_eq!(v1.signatures.len(), 2);
        assert_eq!(v1.tx.fee, 2 * BASE_FEE);
        let Memo::Text(memo) = &v1.tx.memo else {
            panic!("text memo")
        };
        assert_eq!(memo.len(), 28);
        assert_eq!(
            hex::encode(env.hash(network_id(super::super::TESTNET_PASSPHRASE)).unwrap()),
            built.hash
        );
        // Signature verifies against the treasury key.
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&treasury.public_bytes()).unwrap();
        let sig = ed25519_dalek::Signature::from_slice(&v1.signatures[0].signature.0).unwrap();
        vk.verify_strict(&hex::decode(&built.hash).unwrap(), &sig).unwrap();
    }

    #[test]
    fn decodes_delivered_amount_and_failures() {
        let kp = Keypair::random().unwrap();
        let success = TransactionResult {
            fee_charged: 200,
            result: TransactionResultResult::TxSuccess(
                vec![OperationResult::OpInner(OperationResultTr::PathPaymentStrictSend(
                    PathPaymentStrictSendResult::Success(PathPaymentStrictSendResultSuccess {
                        offers: Default::default(),
                        last: SimplePaymentResult {
                            destination: account_id(&kp.public_key()).unwrap(),
                            asset: asset(&ngn(&kp.public_key())).unwrap(),
                            amount: 777,
                        },
                    }),
                ))]
                .try_into()
                .unwrap(),
            ),
            ext: TransactionResultExt::V0,
        };
        let xdr = success.to_xdr_base64(Limits::none()).unwrap();
        assert_eq!(
            decode_result(&xdr, 9),
            TxOutcome::Succeeded {
                ledger: 9,
                delivered: Some(777)
            }
        );

        let failed = TransactionResult {
            fee_charged: 200,
            result: TransactionResultResult::TxFailed(
                vec![OperationResult::OpInner(OperationResultTr::PathPaymentStrictSend(
                    PathPaymentStrictSendResult::UnderDestmin,
                ))]
                .try_into()
                .unwrap(),
            ),
            ext: TransactionResultExt::V0,
        };
        let TxOutcome::Failed { reason } = decode_result(&failed.to_xdr_base64(Limits::none()).unwrap(), 9) else {
            panic!("failed")
        };
        assert!(reason.contains("UnderDestmin"), "{reason}");
    }
}
