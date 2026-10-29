//! Auth, rate limits, LNURL-pay, refunds, payout recovery, the ops dashboard, and the mobile
//! money corridors.

mod common;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use axum::http::{Method, StatusCode};
use common::*;
use satsbridge::{
    adapters::{
        AdapterError,
        lightning::decode_invoice,
        payout::{Institution, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient},
    },
    domain::{Currency, Rail, Recipient},
    services::{payments::PayoutEventStatus, recovery},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// --- auth and limits ---

#[tokio::test]
async fn payments_are_private_to_their_user() {
    let app = app_with(120).await;
    assert_eq!(
        app.raw(Method::GET, "/api/v1/transactions", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.raw(Method::GET, "/api/v1/transactions", Some("sbu_wrong"), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    let (id, _) = app.ngn_payment_awaiting_invoice().await;
    let other = app.new_user().await;
    let (status, _) = app
        .raw(Method::GET, &format!("/api/v1/payments/{id}"), Some(&other), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app
        .raw(Method::POST, &format!("/api/v1/payments/{id}/pay"), Some(&other), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_eq!(
        app.raw(Method::GET, "/api/v1/transactions", Some(&other), None).await.1,
        json!([])
    );
    assert_eq!(
        app.call(Method::GET, "/api/v1/transactions", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Someone else's quote can't be used either.
    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "KES", "amount": 50_000 }))
        .await;
    let body =
        json!({ "quote_id": quote["id"], "recipient": { "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" } });
    assert_eq!(
        app.raw(Method::POST, "/api/v1/payments", Some(&other), Some(body))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn rate_limits_users_and_signups() {
    let app = build(120, |config, _, _| {
        config.rate_limit.per_minute = 3;
        config.rate_limit.signups_per_hour = 2;
    })
    .await;
    for _ in 0..3 {
        assert_eq!(
            app.call(Method::GET, "/api/v1/transactions", None).await.0,
            StatusCode::OK
        );
    }
    let (status, body) = app.call(Method::GET, "/api/v1/transactions", None).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("rate_limited"))
    );

    // The harness already signed up once; one more is allowed this hour.
    app.new_user().await;
    assert_eq!(
        app.raw(Method::POST, "/api/v1/users", None, None).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

// --- LNURL-pay ---

fn lnurl_target(lnurl: &str) -> String {
    let (hrp, data) = bech32::decode(lnurl).unwrap();
    assert!(hrp.as_str().eq_ignore_ascii_case("lnurl"));
    String::from_utf8(data).unwrap()
}

#[tokio::test]
async fn lnurl_pay_serves_the_payments_one_invoice() {
    let app = app_with(120).await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    let lnurl = invoice["lnurl"].as_str().unwrap();
    assert!(lnurl.starts_with("LNURL1"));
    let url = lnurl_target(lnurl);
    assert_eq!(url, format!("{PUBLIC_URL}/api/v1/lnurlp/{id}"));

    let path = url.trim_start_matches(PUBLIC_URL).to_string();
    let (status, pay_request) = app.raw(Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    let msat = invoice["amount_sats"].as_i64().unwrap() * 1000;
    assert_eq!(pay_request["tag"], "payRequest");
    assert_eq!(
        (pay_request["minSendable"].as_i64(), pay_request["maxSendable"].as_i64()),
        (Some(msat), Some(msat))
    );
    assert_eq!(pay_request["callback"], format!("{url}/callback"));

    let (_, wrong) = app
        .raw(Method::GET, &format!("{path}/callback?amount=1000"), None, None)
        .await;
    assert_eq!(wrong["status"], "ERROR");
    let (_, right) = app
        .raw(Method::GET, &format!("{path}/callback?amount={msat}"), None, None)
        .await;
    assert_eq!(right["pr"], invoice["payment_request"], "same invoice as BOLT11");

    // LUD-06: the invoice commits to the metadata the wallet was shown.
    let metadata = pay_request["metadata"].as_str().unwrap();
    let decoded = decode_invoice(right["pr"].as_str().unwrap()).unwrap();
    assert_eq!(decoded.description_hash, Some(hex::encode(Sha256::digest(metadata))));

    app.pay_and_notify(&invoice["payment_hash"]).await;
    assert_eq!(app.raw(Method::GET, &path, None, None).await.1["status"], "ERROR");
    assert!(app.status_of(&id).await["lnurl"].is_null());
}

#[tokio::test]
async fn lnurl_callback_creates_the_invoice_if_needed() {
    let app = app_with(120).await;
    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "KES", "amount": 50_000 }))
        .await;
    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }),
        )
        .await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "QUOTED");
    let path = lnurl_target(details["lnurl"].as_str().unwrap())
        .trim_start_matches(PUBLIC_URL)
        .to_string();
    let msat = quote["amount_sats"].as_i64().unwrap() * 1000;
    let (_, right) = app
        .raw(Method::GET, &format!("{path}/callback?amount={msat}"), None, None)
        .await;
    assert!(right["pr"].as_str().unwrap().starts_with("lnbcrt"));
    assert_eq!(app.status_of(&id).await["status"], "AWAITING_PAYMENT");
}

// --- refunds ---

/// NGN payment whose mock payout then fails: REFUND_PENDING.
async fn refundable_payment(app: &TestApp) -> (String, i64) {
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
    app.mock_payout(&id, "failed").await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING", "{details}");
    (id, invoice["amount_sats"].as_i64().unwrap())
}

#[tokio::test]
async fn failed_payouts_are_refunded_to_the_sender() {
    let app = app_with(120).await;
    let (id, sats) = refundable_payment(&app).await;
    let refund = |pr: &str| json!({ "payment_request": pr });
    let path = format!("/api/v1/payments/{id}/refund");

    let other = app.new_user().await;
    let (good, hash) = bolt11(sats as u64 * 1000, None);
    assert_eq!(
        app.raw(Method::POST, &path, Some(&other), Some(refund(&good))).await.0,
        StatusCode::NOT_FOUND
    );

    let (wrong_amount, _) = bolt11(1_000, None);
    let (status, body) = app.call(Method::POST, &path, Some(refund(&wrong_amount))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = app.call(Method::POST, &path, Some(refund(&good))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "REFUNDED");
    assert_eq!(body["refund_sats"], sats);
    assert_eq!(app.node.paid_out.lock().unwrap().as_slice(), [hash]);

    let (another, _) = bolt11(sats as u64 * 1000, None);
    assert_eq!(
        app.call(Method::POST, &path, Some(refund(&another))).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(app.node.sent(), 1);
}

#[tokio::test]
async fn refunds_with_unknown_outcome_are_resolved_by_recovery() {
    let app = app_with(120).await;
    let (id, sats) = refundable_payment(&app).await;
    let path = format!("/api/v1/payments/{id}/refund");

    // A definite failure releases the lock.
    *app.node.fail_sends.lock().unwrap() = Some(AdapterError::Rejected("no route".into()));
    let (pr, _) = bolt11(sats as u64 * 1000, None);
    assert_eq!(
        app.call(Method::POST, &path, Some(json!({ "payment_request": pr })))
            .await
            .0,
        StatusCode::BAD_GATEWAY
    );
    assert!(app.status_of(&id).await["refund_payment_hash"].is_null());

    // An unknown outcome keeps it, so no second refund can start...
    *app.node.fail_sends.lock().unwrap() = Some(AdapterError::Unavailable("timeout".into()));
    let (pr, hash) = bolt11(sats as u64 * 1000, None);
    let (status, body) = app
        .call(Method::POST, &path, Some(json!({ "payment_request": pr })))
        .await;
    assert_eq!(
        (status, body["status"].as_str()),
        (StatusCode::OK, Some("REFUND_PENDING"))
    );
    assert_eq!(body["refund_payment_hash"], hash);
    let (pr2, _) = bolt11(sats as u64 * 1000, None);
    assert_eq!(
        app.call(Method::POST, &path, Some(json!({ "payment_request": pr2 })))
            .await
            .0,
        StatusCode::CONFLICT
    );

    // ...until recovery sees the payment went through.
    app.node.paid_out.lock().unwrap().push(hash);
    assert_eq!(recovery::run_once(&app.state).await.unwrap().refunds_completed, 1);
    assert_eq!(app.status_of(&id).await["status"], "REFUNDED");
}

// --- payout recovery ---

/// XOF mobile money rail whose behaviour each test sets.
#[derive(Default)]
struct FlakyPayout {
    calls: AtomicUsize,
    /// Number of initial attempts that time out.
    timeouts: usize,
    /// What `find_payout` reports.
    found: Mutex<Option<String>>,
    unsafe_retry: bool,
    references: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl PayoutAdapter for FlakyPayout {
    fn provider(&self) -> &'static str {
        "flaky-mock"
    }

    fn currency(&self) -> Currency {
        Currency::Xof
    }

    fn rail(&self) -> Rail {
        Rail::MobileMoney
    }

    fn safe_to_retry(&self) -> bool {
        !self.unsafe_retry
    }

    async fn institutions(&self, _: Option<&str>) -> Result<Vec<Institution>, AdapterError> {
        Ok(vec![])
    }

    async fn resolve_recipient(&self, _: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        Ok(ResolvedRecipient {
            account_name: Some("AWA DIOP".into()),
            ..Default::default()
        })
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.timeouts {
            return Err(AdapterError::Unavailable("timeout".into()));
        }
        let reference = format!("flaky_{}_a{}", request.idempotency_key, request.attempt);
        self.references.lock().unwrap().push(reference.clone());
        Ok(PayoutAccepted {
            provider_reference: reference,
            completed: false,
        })
    }

    async fn find_payout(&self, _: &str) -> Result<Option<PayoutAccepted>, AdapterError> {
        Ok(self.found.lock().unwrap().clone().map(|r| PayoutAccepted {
            provider_reference: r,
            completed: false,
        }))
    }

    async fn payout_status(&self, _: &str) -> Result<Option<PayoutEventStatus>, AdapterError> {
        Ok(Some(PayoutEventStatus::Success))
    }
}

async fn flaky_app(flaky: Arc<FlakyPayout>, tweak: impl FnOnce(&mut satsbridge::Config)) -> TestApp {
    build(120, move |config, payouts, _| {
        config.recovery.base_backoff_secs = 0;
        tweak(config);
        payouts.register(flaky);
    })
    .await
}

/// XOF -> XOF isn't allowed, so pay XAF in and XOF out.
async fn settled_xof_payment(app: &TestApp) -> String {
    let quote = app
        .quote(json!({ "source_currency": "XAF", "destination_currency": "XOF", "amount": 50_000, "rail": "MOBILE_MONEY" }))
        .await;
    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "MOBILE_MONEY", "country": "SN", "provider_code": "WAVE", "phone_number": "771234567" }),
        )
        .await;
    let invoice = app.invoice_for(&id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
    id
}

async fn payout_of(app: &TestApp, id: &str) -> Value {
    app.status_of(id).await["payout"].clone()
}

#[tokio::test]
async fn recovery_resubmits_after_an_unconfirmed_attempt() {
    let flaky = Arc::new(FlakyPayout {
        timeouts: 1,
        ..Default::default()
    });
    let app = flaky_app(flaky.clone(), |_| {}).await;
    let id = settled_xof_payment(&app).await;
    let payout = payout_of(&app, &id).await;
    assert_eq!(
        (payout["status"].as_str(), payout["attempts"].as_i64()),
        (Some("pending"), Some(1))
    );

    let report = recovery::run_once(&app.state).await.unwrap();
    assert_eq!(report.resubmitted, 1);
    let payout = payout_of(&app, &id).await;
    assert_eq!(payout["status"], "submitted");
    assert_eq!(payout["provider_reference"], format!("flaky_{id}_a2"));
    assert_eq!(flaky.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn recovery_reuses_an_earlier_accepted_attempt() {
    let flaky = Arc::new(FlakyPayout {
        timeouts: 1,
        ..Default::default()
    });
    *flaky.found.lock().unwrap() = Some("accepted-earlier".into());
    let app = flaky_app(flaky.clone(), |_| {}).await;
    let id = settled_xof_payment(&app).await;

    assert_eq!(recovery::run_once(&app.state).await.unwrap().reconciled, 1);
    assert_eq!(payout_of(&app, &id).await["provider_reference"], "accepted-earlier");
    assert_eq!(flaky.calls.load(Ordering::SeqCst), 1, "never paid twice");
}

#[tokio::test]
async fn recovery_waits_for_backoff_and_gives_up_after_max_attempts() {
    let flaky = Arc::new(FlakyPayout {
        timeouts: 10,
        ..Default::default()
    });
    let app = flaky_app(flaky.clone(), |c| c.recovery.max_attempts = 2).await;
    let id = settled_xof_payment(&app).await;

    assert_eq!(recovery::run_once(&app.state).await.unwrap().resubmitted, 1);
    let report = recovery::run_once(&app.state).await.unwrap();
    assert_eq!(report.flagged, 1);
    let payout = payout_of(&app, &id).await;
    assert_eq!(payout["needs_review"], true);
    assert!(
        payout["review_reason"]
            .as_str()
            .unwrap()
            .contains("gave up after 2 attempts")
    );

    // Default backoff: nothing is retried immediately.
    let flaky = Arc::new(FlakyPayout {
        timeouts: 1,
        ..Default::default()
    });
    let app = build(120, |_, payouts, _| payouts.register(flaky.clone())).await;
    settled_xof_payment(&app).await;
    assert_eq!(
        recovery::run_once(&app.state).await.unwrap(),
        recovery::RecoveryReport::default()
    );
}

#[tokio::test]
async fn unsafe_rails_go_to_review_and_admins_decide() {
    let flaky = Arc::new(FlakyPayout {
        timeouts: 1,
        unsafe_retry: true,
        ..Default::default()
    });
    let app = flaky_app(flaky.clone(), |_| {}).await;
    let id = settled_xof_payment(&app).await;
    assert_eq!(payout_of(&app, &id).await["needs_review"], true);
    assert_eq!(
        recovery::run_once(&app.state).await.unwrap(),
        recovery::RecoveryReport::default()
    );

    let (_, summary) = app.admin(Method::GET, "/api/v1/admin/summary", None).await;
    assert_eq!(summary["needs_review"], 1);
    let (_, listed) = app
        .admin(Method::GET, "/api/v1/admin/payments?needs_review=true", None)
        .await;
    assert_eq!(listed[0]["id"], id);

    let retry = format!("/api/v1/admin/payments/{id}/retry-payout");
    assert_eq!(
        app.admin(Method::POST, &retry, None).await.1["outcome"],
        "flagged for review"
    );
    let (status, body) = app.admin(Method::POST, &retry, Some(json!({ "force": true }))).await;
    assert_eq!(
        (status, body["outcome"].as_str()),
        (StatusCode::OK, Some("resubmitted"))
    );
    assert_eq!(payout_of(&app, &id).await["status"], "submitted");
}

#[tokio::test]
async fn admin_can_fail_a_stuck_payout_into_refund() {
    let flaky = Arc::new(FlakyPayout {
        timeouts: 1,
        unsafe_retry: true,
        ..Default::default()
    });
    let app = flaky_app(flaky, |_| {}).await;
    let id = settled_xof_payment(&app).await;
    let (status, body) = app
        .admin(
            Method::POST,
            &format!("/api/v1/admin/payments/{id}/fail-payout"),
            Some(json!({ "reason": "checked with provider" })),
        )
        .await;
    assert_eq!(
        (status, body["status"].as_str()),
        (StatusCode::OK, Some("REFUND_PENDING")),
        "{body}"
    );
}

#[tokio::test]
async fn recovery_asks_providers_about_unconfirmed_payouts() {
    let flaky = Arc::new(FlakyPayout::default());
    let app = flaky_app(flaky, |c| c.recovery.unconfirmed_after_secs = 0).await;
    let id = settled_xof_payment(&app).await;
    assert_eq!(payout_of(&app, &id).await["status"], "submitted");
    assert_eq!(recovery::run_once(&app.state).await.unwrap().confirmed, 1);
    assert_eq!(app.status_of(&id).await["status"], "COMPLETED");
}

// --- ops dashboard ---

#[tokio::test]
async fn admin_summary_reports_metrics() {
    let app = app_with(120).await;
    assert_eq!(
        app.raw(Method::GET, "/api/v1/admin/summary", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.call(Method::GET, "/api/v1/admin/summary", None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let (done, invoice) = app.ngn_payment_awaiting_invoice().await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
    app.mock_payout(&done, "success").await;
    refundable_payment(&app).await;

    let (status, summary) = app.admin(Method::GET, "/api/v1/admin/summary", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(summary["total_payments"], 2);
    assert_eq!(summary["completion_rate"], 0.5);
    assert_eq!(summary["refund_pending"], 1);
    assert_eq!(summary["corridors"][0]["source_currency"], "XOF");
    assert_eq!(summary["corridors"][0]["completed"], 1);
    assert!(summary["avg_settlement_secs"].is_number());
    assert!(
        summary["rails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rail"] == "MOBILE_MONEY" && r["currency"] == "XAF")
    );

    let (_, refund_pending) = app
        .admin(Method::GET, "/api/v1/admin/payments?status=REFUND_PENDING", None)
        .await;
    assert_eq!(refund_pending.as_array().unwrap().len(), 1);
    assert_eq!(
        app.admin(Method::POST, "/api/v1/admin/recovery/run", None).await.0,
        StatusCode::OK
    );
}

// --- mobile money corridors ---

#[tokio::test]
async fn xof_to_xaf_mobile_money() {
    let app = app_with(120).await;
    let (_, operators) = app
        .raw(
            Method::GET,
            "/api/v1/payout-institutions?currency=XAF&rail=MOBILE_MONEY&country=CM",
            None,
            None,
        )
        .await;
    assert!(operators.as_array().unwrap().iter().any(|o| o["code"] == "MTN"));

    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "XAF", "amount": 100_000 }))
        .await;
    assert_eq!(quote["destination_rail"], "MOBILE_MONEY", "only rail for XAF");

    let wrong_country =
        json!({ "rail": "MOBILE_MONEY", "country": "SN", "provider_code": "MTN", "phone_number": "677123456" });
    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": wrong_country })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "MOBILE_MONEY", "country": "CM", "provider_code": "MTN", "phone_number": "677123456", "account_name": "Jean Mbarga" }),
        )
        .await;
    let invoice = app.invoice_for(&id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
    app.mock_payout(&id, "success").await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "COMPLETED");
    assert_eq!(details["recipient_name"], "Jean Mbarga");
    assert_eq!(details["recipient_masked"], "CM:MTN:*****3456");
}

#[tokio::test]
async fn xof_to_gnf_mobile_money() {
    let app = app_with(120).await;
    let (_, operators) = app
        .raw(
            Method::GET,
            "/api/v1/payout-institutions?currency=GNF&rail=MOBILE_MONEY&country=GN",
            None,
            None,
        )
        .await;
    assert!(operators.as_array().unwrap().iter().any(|o| o["code"] == "ORANGE"));

    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "GNF", "amount": 100_000 }))
        .await;
    assert_eq!(quote["destination_rail"], "MOBILE_MONEY", "only rail for GNF");

    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "MOBILE_MONEY", "country": "GN", "provider_code": "ORANGE", "phone_number": "622123456", "account_name": "Mamadou Diallo" }),
        )
        .await;
    let invoice = app.invoice_for(&id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
    app.mock_payout(&id, "success").await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "COMPLETED");
    assert_eq!(details["recipient_masked"], "GN:ORANGE:*****3456");
}
