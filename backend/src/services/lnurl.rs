//! LNURL-pay (LUD-06) for senders: a wallet scans `LNURL1...`, fetches the pay request, and
//! asks the callback for an invoice. The callback returns the payment's one invoice, which
//! commits to [`metadata`], so LNURL and plain BOLT11 can't produce two payable invoices.

use bech32::{Bech32, Hrp};
use serde_json::{Value, json};

use super::payments;
use crate::{
    db::{self, PaymentIntent},
    domain::{PaymentStatus, money::minor_to_major},
    state::AppState,
};

/// LUD-06 metadata. Derived only from fields fixed at payment creation, so it is stable.
pub fn metadata(payment: &PaymentIntent) -> String {
    let text = format!(
        "SatsBridge {}: {} {} to {}",
        payment.id,
        minor_to_major(payment.source_amount, payment.source_currency),
        payment.source_currency,
        payment.destination_currency
    );
    json!([["text/plain", text]]).to_string()
}

pub fn pay_request_url(state: &AppState, payment_id: &str) -> Option<String> {
    let base = state.config.public_base_url.as_deref()?;
    Some(format!("{base}/api/v1/lnurlp/{payment_id}"))
}

/// Bech32 `LNURL1...` (upper case, for compact QR codes).
pub fn encode(url: &str) -> Option<String> {
    let hrp = Hrp::parse("lnurl").ok()?;
    bech32::encode::<Bech32>(hrp, url.as_bytes())
        .ok()
        .map(|s| s.to_uppercase())
}

pub fn for_payment(state: &AppState, payment: &PaymentIntent) -> Option<String> {
    matches!(payment.status, PaymentStatus::Quoted | PaymentStatus::AwaitingPayment)
        .then(|| pay_request_url(state, &payment.id))
        .flatten()
        .and_then(|url| encode(&url))
}

fn error(reason: impl Into<String>) -> Value {
    json!({ "status": "ERROR", "reason": reason.into() })
}

async fn payable(state: &AppState, payment_id: &str) -> Result<PaymentIntent, Value> {
    let payment = db::get_payment(&state.pool, payment_id)
        .await
        .map_err(|_| error("temporarily unavailable"))?
        .ok_or_else(|| error("unknown payment"))?;
    if !matches!(payment.status, PaymentStatus::Quoted | PaymentStatus::AwaitingPayment) {
        return Err(error(format!("payment is {}", payment.status.as_str())));
    }
    Ok(payment)
}

/// `GET /lnurlp/{id}`: the fixed-amount pay request.
pub async fn pay_request(state: &AppState, payment_id: &str) -> Value {
    let payment = match payable(state, payment_id).await {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some(url) = pay_request_url(state, payment_id) else {
        return error("LNURL is not configured");
    };
    let msat = payment.amount_sats * 1000;
    json!({
        "tag": "payRequest",
        "callback": format!("{url}/callback"),
        "minSendable": msat,
        "maxSendable": msat,
        "metadata": metadata(&payment),
        "commentAllowed": 0,
    })
}

/// `GET /lnurlp/{id}/callback?amount=<msat>`: returns the payment's invoice.
pub async fn callback(state: &AppState, payment_id: &str, amount_msat: Option<i64>) -> Value {
    let payment = match payable(state, payment_id).await {
        Ok(p) => p,
        Err(e) => return e,
    };
    if amount_msat != Some(payment.amount_sats * 1000) {
        return error(format!("amount must be exactly {} msat", payment.amount_sats * 1000));
    }
    match payments::invoice_for(state, &payment).await {
        Ok(invoice) => json!({ "pr": invoice.payment_request, "routes": [] }),
        Err(err) => error(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_lnurl_as_bech32() {
        // LUD-01 example.
        let url = "https://service.com/api?q=3fc3645b439ce8e7f2553a69e5267081d96dcd340693afabe04be7b0ccd178df";
        assert_eq!(
            encode(url).unwrap(),
            "LNURL1DP68GURN8GHJ7UM9WFMXJCM99E3K7MF0V9CXJ0M385EKVCENXC6R2C35XVUKXEFCV5MKVV34X5EKZD3EV56NYD3HXQURZEPEXEJXXEPNXSCRVWFNV9NXZCN9XQ6XYEFHVGCXXCMYXYMNSERXFQ5FNS"
        );
    }
}
