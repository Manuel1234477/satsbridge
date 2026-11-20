//! Auth, rate limits, payout recovery, the ops dashboard, and the mobile money corridors.

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
        payout::{Institution, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient},
    },
    domain::{Currency, Rail, Recipient},
    services::{payments::PayoutEventStatus, recovery},
};
use serde_json::{Value, json};

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

    let (id, _) = app.kes_payment_awaiting_deposit().await;
    let other = app.new_user().await;
    let (status, _) = app
        .raw(Method::GET, &format!("/api/v1/payments/{id}"), Some(&other), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    for path in ["pay", "refund"] {
        let (status, _) = app
            .raw(
                Method::POST,
                &format!("/api/v1/payments/{id}/{path}"),
                Some(&other),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (status, _) = app
        .raw(
            Method::POST,
            &format!("/api/v1/dev/payments/{id}/deposit"),
            Some(&other),
            None,
        )
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
        .quote(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": SEND_KOBO }))
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
    let app = build(120, |config, _| {
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
    build(120, move |config, payouts| {
        config.recovery.base_backoff_secs = 0;
        tweak(config);
        payouts.register(flaky);
    })
    .await
}

/// ₦150,000 -> XOF mobile money, deposited and settled on Stellar.
async fn settled_xof_payment(app: &TestApp) -> String {
    let quote = app
        .quote(json!({ "source_currency": "NGN", "destination_currency": "XOF", "amount": SEND_KOBO, "rail": "MOBILE_MONEY" }))
        .await;
    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "MOBILE_MONEY", "country": "SN", "provider_code": "WAVE", "phone_number": "771234567" }),
        )
        .await;
    app.pay(&id).await;
    app.deposit(&id, None).await;
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
    let app = build(120, |_, payouts| payouts.register(flaky.clone())).await;
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
    // The conversion is reversed on Stellar and the sender refunded automatically.
    let details = app.status_of(&id).await;
    assert_eq!(
        stellar_steps(&details),
        steps(&[("settle", "succeeded"), ("reverse", "succeeded")])
    );
    assert_eq!(details["refund"]["status"], "submitted");
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

    let (done, _) = app.kes_payment_awaiting_deposit().await;
    app.deposit(&done, None).await;
    app.mock_payout(&done, "success").await;
    let (failed, _) = app.kes_payment_awaiting_deposit().await;
    app.deposit(&failed, None).await;
    app.mock_payout(&failed, "failed").await;

    let (status, summary) = app.admin(Method::GET, "/api/v1/admin/summary", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(summary["total_payments"], 2);
    assert_eq!(summary["completion_rate"], 0.5);
    assert_eq!(summary["refund_pending"], 1);
    assert_eq!(summary["corridors"][0]["source_currency"], "NGN");
    assert_eq!(summary["corridors"][0]["destination_currency"], "KES");
    assert_eq!(summary["corridors"][0]["completed"], 1);
    assert!(summary["avg_settlement_secs"].is_number(), "{summary}");
    assert_eq!(summary["ledger"], "simulated");
    assert_eq!(summary["collections"], json!(["NGN"]));
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

/// Quote ₦150,000 into `currency`, pay to `recipient`, deposit, and complete the payout.
async fn complete_mobile_money(app: &TestApp, currency: &str, recipient: Value) -> Value {
    let quote = app
        .quote(json!({ "source_currency": "NGN", "destination_currency": currency, "amount": SEND_KOBO }))
        .await;
    assert_eq!(quote["destination_rail"], "MOBILE_MONEY", "only rail for {currency}");
    assert_eq!(quote["conversion_path"], json!(["NGN", "USD", currency]));
    let id = app.create_payment(&quote, recipient).await;
    app.pay(&id).await;
    app.deposit(&id, None).await;
    app.mock_payout(&id, "success").await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "COMPLETED", "{details}");
    details
}

#[tokio::test]
async fn ngn_to_xaf_mobile_money() {
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
        .quote(json!({ "source_currency": "NGN", "destination_currency": "XAF", "amount": SEND_KOBO }))
        .await;
    // ₦148,500 = $99 = 59,400 XAF, less the 0.5% buffer.
    assert_eq!(quote["destination_amount"], 59_103);
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

    let details = complete_mobile_money(
        &app,
        "XAF",
        json!({ "rail": "MOBILE_MONEY", "country": "CM", "provider_code": "MTN", "phone_number": "677123456", "account_name": "Jean Mbarga" }),
    )
    .await;
    assert_eq!(details["recipient_name"], "Jean Mbarga");
    assert_eq!(details["recipient_masked"], "CM:MTN:*****3456");
}

#[tokio::test]
async fn ngn_to_gnf_and_ghs_mobile_money() {
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

    let details = complete_mobile_money(
        &app,
        "GNF",
        json!({ "rail": "MOBILE_MONEY", "country": "GN", "provider_code": "ORANGE", "phone_number": "622123456", "account_name": "Mamadou Diallo" }),
    )
    .await;
    assert_eq!(details["recipient_masked"], "GN:ORANGE:*****3456");

    let (_, currencies) = app.raw(Method::GET, "/api/v1/currencies", None, None).await;
    let ghs = currencies
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "GHS")
        .unwrap();
    assert_eq!(ghs["payout_rails"][0]["countries"], json!(["GH"]));
    let details = complete_mobile_money(
        &app,
        "GHS",
        json!({ "rail": "MOBILE_MONEY", "country": "GH", "provider_code": "MTN", "phone_number": "0241234567", "account_name": "Kwame Mensah" }),
    )
    .await;
    // ₦148,500 = $99 = GH₵1,485, less the buffer.
    assert_eq!(details["expected_payout"], 147_757);
    assert_eq!(details["destination_currency"], "GHS");
}
