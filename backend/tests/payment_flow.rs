//! End-to-end payment flows: Naira in by bank transfer, settled on Stellar, paid out locally.

mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::*;
use serde_json::json;
use stellarbridge::{
    adapters::stellar::simulated::Inject, domain::Currency, security::SIGNATURE_HEADER, services::recovery,
};

#[tokio::test]
async fn ngn_to_kes_settles_on_stellar_and_completes() {
    let app = app_with(120).await;
    let quote = app
        .quote(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": SEND_KOBO }))
        .await;
    assert_eq!(quote["fee_source_amount"], 150_000);
    assert_eq!(quote["send_amount"], 14_850_000);
    assert_eq!(quote["destination_amount"], PAYOUT_KES_CENTS);
    assert_eq!(quote["conversion_path"], json!(["NGN", "USD", "KES"]));
    assert_eq!(quote["rate_source"], "simulated");

    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }),
        )
        .await;
    let deposit = app.pay(&id).await;
    assert_eq!(deposit["expected_amount"], SEND_KOBO);
    assert_eq!(deposit["account_number"].as_str().unwrap().len(), 10);
    assert_eq!(deposit["status"], "awaiting");
    // Asking again returns the same account.
    assert_eq!(app.pay(&id).await["account_number"], deposit["account_number"]);
    assert_eq!(app.status_of(&id).await["status"], "AWAITING_PAYMENT");

    // The collection provider reports the transfer.
    let event = json!({
        "event_id": "dep-1",
        "reference": deposit["reference"],
        "amount_minor": SEND_KOBO,
        "currency": "NGN",
        "sender": { "bank_code": "058", "account_number": "0123456789", "account_name": "ADA OBI" },
    });
    let (status, body) = app
        .webhook("/api/v1/webhooks/collections", PROVIDER_SECRET, event.clone())
        .await;
    assert_eq!(
        (status, body["outcome"].as_str()),
        (StatusCode::OK, Some("processed")),
        "{body}"
    );
    let (_, body) = app
        .webhook("/api/v1/webhooks/collections", PROVIDER_SECRET, event)
        .await;
    assert_eq!(body["outcome"], "duplicate");

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING", "{details}");
    assert_eq!(details["ledger"], "simulated");
    assert_eq!(details["deposit"]["received_amount"], SEND_KOBO);
    assert_eq!(stellar_steps(&details), steps(&[("settle", "succeeded")]));
    let settle = &details["stellar_transactions"][0];
    assert_eq!(settle["delivered"], DELIVERED_KES_STROOPS);
    // ₦150,000 and ₦148,500 in stroops (7 decimals).
    assert_eq!(settle["operation"]["mint"], 1_500_000_000_000_i64);
    assert_eq!(settle["operation"]["send"], 1_485_000_000_000_i64);
    assert_eq!(settle["operation"]["dest_min"], 128_056_500_000_i64);
    // In flight: the KES tokens and the ₦1,500 fee sit in the treasury.
    assert_eq!(app.ledger.treasury_balance(Currency::Kes), DELIVERED_KES_STROOPS);
    assert_eq!(app.ledger.treasury_balance(Currency::Ngn), 15_000_000_000);
    assert_eq!(details["payout"]["destination_amount"], PAYOUT_KES_CENTS);
    assert_eq!(details["payout"]["attempts"], 1);

    let (status, _) = app.mock_payout(&id, "success").await;
    assert_eq!(status, StatusCode::OK);
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "COMPLETED");
    // The payout is done, so the KES tokens are burned.
    assert_eq!(
        stellar_steps(&details),
        steps(&[("settle", "succeeded"), ("redeem", "succeeded")])
    );
    assert_eq!(app.ledger.treasury_balance(Currency::Kes), 0);
    assert_eq!(
        to_statuses(&details),
        [
            "CREATED",
            "QUOTED",
            "AWAITING_PAYMENT",
            "PAYMENT_DETECTED",
            "SETTLED",
            "PAYOUT_PENDING",
            "COMPLETED"
        ]
    );

    // Nothing left to do: recovery changes nothing and submits nothing.
    let before = app.ledger.submissions();
    recovery::run_once(&app.state).await.unwrap();
    assert_eq!(app.ledger.submissions(), before);
}

#[tokio::test]
async fn failed_payout_is_reversed_on_stellar_and_refunded_to_the_sender() {
    let app = app_with(120).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    app.deposit(&id, None).await;
    app.mock_payout(&id, "failed").await;

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING", "{details}");
    assert_eq!(
        stellar_steps(&details),
        steps(&[("settle", "succeeded"), ("reverse", "succeeded")])
    );
    // The conversion is undone and the fee burned: the treasury holds nothing for this payment.
    assert_eq!(app.ledger.treasury_balance(Currency::Kes), 0);
    assert_eq!(app.ledger.treasury_balance(Currency::Ngn), 0);

    // Refunded automatically, in full, to the account the transfer came from.
    let refund = &details["refund"];
    assert_eq!(refund["status"], "submitted", "{details}");
    assert_eq!(refund["amount"], SEND_KOBO);
    assert_eq!(refund["currency"], "NGN");
    assert_eq!(refund["destination_reference"], "058:******6789");
    assert_eq!(app.ngn.submissions(), 1);

    app.mock_refund(&id, "success").await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUNDED");
    assert_eq!(details["refund"]["status"], "completed");
}

#[tokio::test]
async fn underpaid_deposit_is_refunded_without_touching_stellar() {
    let app = app_with(120).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    app.deposit(&id, Some(json!({ "amount_minor": 10_000_000 }))).await;

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING");
    assert!(
        details["failure_reason"]
            .as_str()
            .unwrap()
            .contains("received 10000000")
    );
    assert!(stellar_steps(&details).is_empty());
    assert_eq!(app.ledger.submissions(), 0);
    assert_eq!(details["refund"]["amount"], 10_000_000);
}

#[tokio::test]
async fn refund_needs_an_account_when_the_source_is_unknown() {
    let app = app_with(120).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    app.deposit(&id, Some(json!({ "amount_minor": 1_000, "sender": null })))
        .await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING");
    assert!(details["refund"].is_null());

    let refund_url = format!("/api/v1/payments/{id}/refund");
    let (status, _) = app.call(Method::POST, &refund_url, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Without a resolution the account isn't accepted.
    let unresolved = json!({ "bank_code": "044", "account_number": "1112223334", "resolution_id": "res_x" });
    assert_eq!(
        app.call(Method::POST, &refund_url, Some(unresolved)).await.0,
        StatusCode::BAD_REQUEST
    );

    let (_, resolved) = app
        .call(
            Method::POST,
            "/api/v1/recipients/resolve",
            Some(json!({ "currency": "NGN", "bank_code": "044", "account_number": "1112223334" })),
        )
        .await;
    let (status, refund) = app
        .call(
            Method::POST,
            &refund_url,
            Some(json!({ "bank_code": "044", "account_number": "1112223334", "resolution_id": resolved["resolution_id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{refund}");
    assert_eq!(refund["status"], "submitted");
    assert_eq!(refund["account_name"], "DEMO RECIPIENT 3334");

    // Can't start a second one while this is in flight.
    let (status, _) = app
        .call(
            Method::POST,
            &refund_url,
            Some(json!({ "bank_code": "044", "account_number": "1112223334", "resolution_id": resolved["resolution_id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // A failed refund can be retried to another account.
    app.mock_refund(&id, "failed").await;
    assert_eq!(app.status_of(&id).await["refund"]["status"], "failed");
    let (status, refund) = app
        .call(
            Method::POST,
            &refund_url,
            Some(json!({ "bank_code": "044", "account_number": "1112223334", "resolution_id": resolved["resolution_id"] })),
        )
        .await;
    assert_eq!((status, refund["status"].as_str()), (StatusCode::OK, Some("submitted")));
    app.mock_refund(&id, "success").await;
    assert_eq!(app.status_of(&id).await["status"], "REFUNDED");
}

#[tokio::test]
async fn dex_moving_below_the_quote_fails_settlement_and_refunds() {
    let app = app_with(120).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    // The DEX now delivers 2% less: below the quoted destMin (0.5% buffer).
    app.ledger.set_price_shift_bps(-200);
    app.deposit(&id, None).await;

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING", "{details}");
    assert!(details["failure_reason"].as_str().unwrap().contains("UnderDestmin"));
    assert_eq!(stellar_steps(&details), steps(&[("settle", "failed")]));
    assert!(details["payout"].is_null());
    assert_eq!(
        app.ledger.treasury_balance(Currency::Ngn),
        0,
        "atomic: nothing was minted"
    );
    assert_eq!(details["refund"]["amount"], SEND_KOBO);
}

#[tokio::test]
async fn stellar_timeouts_are_resolved_by_recovery() {
    let app = build(120, |c, _| c.recovery.stellar_confirm_after_secs = 0).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    // Applied on the ledger, but our submission timed out.
    app.ledger.inject(Inject::ApplyButTimeout);
    app.deposit(&id, None).await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYMENT_DETECTED");
    assert_eq!(stellar_steps(&details), steps(&[("settle", "submitted")]));

    let report = recovery::run_once(&app.state).await.unwrap();
    assert_eq!(report.stellar_resolved, 1);
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING");
    assert_eq!(stellar_steps(&details), steps(&[("settle", "succeeded")]));
    assert_eq!(
        app.ledger.treasury_balance(Currency::Kes),
        DELIVERED_KES_STROOPS,
        "applied once"
    );
}

#[tokio::test]
async fn lost_stellar_transactions_are_rebuilt_only_after_they_expire() {
    let app = build(120, |c, _| c.recovery.stellar_confirm_after_secs = 0).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    // Never reached the network.
    app.ledger.inject(Inject::DropAndTimeout);
    app.deposit(&id, None).await;
    assert_eq!(app.status_of(&id).await["status"], "PAYMENT_DETECTED");

    // Still inside its time bounds: recovery resubmits the same envelope, never a new one.
    recovery::run_once(&app.state).await.unwrap();
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING");
    assert_eq!(stellar_steps(&details), steps(&[("settle", "succeeded")]));

    // Same again, but the lost transaction's maxTime has passed: it is retired and rebuilt.
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    app.ledger.inject(Inject::DropAndTimeout);
    app.deposit(&id, None).await;
    sqlx::query("UPDATE stellar_transactions SET valid_until = ? WHERE payment_intent_id = ?")
        .bind(chrono::Utc::now() - chrono::Duration::minutes(5))
        .bind(&id)
        .execute(&app.state.pool)
        .await
        .unwrap();
    recovery::run_once(&app.state).await.unwrap();
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING", "{details}");
    assert_eq!(
        stellar_steps(&details),
        steps(&[("settle", "failed"), ("settle", "succeeded")])
    );
}

#[tokio::test]
async fn deposits_after_expiry_are_refunded() {
    let app = build(120, |c, _| c.collections.deposit_expiry_secs = 0).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    assert_eq!(recovery::run_once(&app.state).await.unwrap().expired, 1);
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "EXPIRED");
    assert_eq!(details["deposit"]["status"], "expired");

    // The sender paid anyway.
    app.deposit(&id, None).await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING");
    assert_eq!(details["refund"]["status"], "submitted");
    assert!(stellar_steps(&details).is_empty());
}

#[tokio::test]
async fn rejects_bad_signatures_and_unsupported_corridors() {
    let app = app_with(120).await;
    for path in ["/api/v1/webhooks/provider", "/api/v1/webhooks/collections"] {
        let req = Request::post(path)
            .header(SIGNATURE_HEADER, "deadbeef")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(app.send(req).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    // There is no endpoint that settles a payment on the caller's word.
    let (status, _) = app.call(Method::POST, "/api/v1/dev/payments/x/settle", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    for (body, why) in [
        (
            json!({ "source_currency": "KES", "destination_currency": "NGN", "amount": 100_000 }),
            "no KES collection",
        ),
        (
            json!({ "source_currency": "NGN", "destination_currency": "NGN", "amount": 100_000 }),
            "same currency",
        ),
        (
            json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": 0 }),
            "zero",
        ),
        (
            json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": 1 }),
            "below fees",
        ),
    ] {
        let (status, body) = app.call(Method::POST, "/api/v1/quotes", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
    }

    // NGN is the default source currency.
    let quote = app
        .quote(json!({ "destination_currency": "KES", "amount": SEND_KOBO }))
        .await;
    assert_eq!(quote["source_currency"], "NGN");

    // A deposit for an unknown reference is refused (and retried by the provider).
    let event = json!({ "event_id": "x", "reference": "nope", "amount_minor": 1, "currency": "NGN" });
    let (status, _) = app
        .webhook("/api/v1/webhooks/collections", PROVIDER_SECRET, event)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn expired_quotes_and_limits_are_enforced() {
    let app = app_with(0).await;
    let quote = app
        .quote(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": SEND_KOBO }))
        .await;
    let (status, body) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": { "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" } })),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::GONE, Some("quote_expired"))
    );

    // ₦6,000,000 is above the ₦5,000,000 deposit limit.
    let (status, body) = app
        .call(
            Method::POST,
            "/api/v1/quotes",
            Some(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": 600_000_000 })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (_, currencies) = app.raw(Method::GET, "/api/v1/currencies", None, None).await;
    let find = |code: &str| {
        currencies
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["code"] == code)
            .unwrap()
            .clone()
    };
    assert_eq!(find("NGN")["can_send"], true);
    assert_eq!(find("NGN")["stellar_asset"]["code"], "NGN");
    assert_eq!(find("KES")["can_send"], false);
    assert_eq!(find("KES")["payout_rails"][0]["rail"], "KE_MOBILE_MONEY");
    assert_eq!(find("GHS")["payout_rails"][0]["rail"], "MOBILE_MONEY");
}

#[tokio::test]
async fn destination_side_quotes_deliver_the_exact_amount() {
    let app = app_with(120).await;
    let quote = app
        .quote(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": 1_000_000, "amount_side": "destination" }))
        .await;
    assert_eq!(quote["destination_amount"], 1_000_000);
    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }),
        )
        .await;
    app.pay(&id).await;
    app.deposit(&id, None).await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING", "{details}");
    assert!(details["stellar_transactions"][0]["delivered"].as_i64().unwrap() >= 100_000_000_000);
}

#[tokio::test]
async fn payout_reversal_after_completion_reopens_for_refund() {
    let app = app_with(120).await;
    let (id, _) = app.kes_payment_awaiting_deposit().await;
    app.deposit(&id, None).await;
    let reference = app.status_of(&id).await["payout"]["provider_reference"].clone();
    app.mock_payout(&id, "success").await;
    assert_eq!(app.status_of(&id).await["status"], "COMPLETED");

    let reversed = json!({ "event_id": "po-r", "provider_reference": reference, "status": "reversed" });
    app.webhook("/api/v1/webhooks/provider", PROVIDER_SECRET, reversed)
        .await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING");
    assert_eq!(details["payout"]["status"], "reversed");
    // The tokens were already burned on completion, so there is nothing to reverse on-chain.
    assert_eq!(
        stellar_steps(&details),
        steps(&[("settle", "succeeded"), ("redeem", "succeeded")])
    );
    assert_eq!(details["refund"]["status"], "submitted");
}
