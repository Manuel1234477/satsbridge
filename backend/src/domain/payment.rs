use serde::{Deserialize, Serialize};

/// Payment lifecycle from PRD §11, plus the NGN reversal rule from the NGN addendum (A.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaymentStatus {
    Created,
    Quoted,
    AwaitingPayment,
    PaymentDetected,
    Settled,
    PayoutPending,
    Completed,
    Expired,
    Failed,
    RefundPending,
    Refunded,
}

impl PaymentStatus {
    pub fn can_transition_to(self, next: PaymentStatus) -> bool {
        use PaymentStatus::*;
        matches!(
            (self, next),
            (Created, Quoted | Expired | Failed)
                | (Quoted, AwaitingPayment | Expired | Failed)
                | (AwaitingPayment, PaymentDetected | Settled | Expired | Failed)
                | (PaymentDetected, Settled | Failed)
                | (Settled, PayoutPending | Failed)
                | (PayoutPending, Completed | Failed)
                // Only for a provider reversal after a reported success.
                | (Completed, Failed)
                | (Failed, RefundPending)
                | (RefundPending, Refunded)
        )
    }

    pub fn as_str(self) -> &'static str {
        use PaymentStatus::*;
        match self {
            Created => "CREATED",
            Quoted => "QUOTED",
            AwaitingPayment => "AWAITING_PAYMENT",
            PaymentDetected => "PAYMENT_DETECTED",
            Settled => "SETTLED",
            PayoutPending => "PAYOUT_PENDING",
            Completed => "COMPLETED",
            Expired => "EXPIRED",
            Failed => "FAILED",
            RefundPending => "REFUND_PENDING",
            Refunded => "REFUNDED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PaymentStatus::*;

    #[test]
    fn happy_path_is_allowed() {
        let path = [Created, Quoted, AwaitingPayment, Settled, PayoutPending, Completed];
        for pair in path.windows(2) {
            assert!(pair[0].can_transition_to(pair[1]), "{:?} -> {:?}", pair[0], pair[1]);
        }
    }

    #[test]
    fn terminal_and_backward_moves_are_rejected() {
        assert!(!Completed.can_transition_to(PayoutPending));
        assert!(!Expired.can_transition_to(AwaitingPayment));
        assert!(!Settled.can_transition_to(AwaitingPayment));
        assert!(!Refunded.can_transition_to(Completed));
        assert!(!Quoted.can_transition_to(Settled));
    }

    #[test]
    fn reversal_path_goes_through_failed() {
        assert!(Completed.can_transition_to(Failed));
        assert!(Failed.can_transition_to(RefundPending));
        assert!(!Completed.can_transition_to(RefundPending));
    }
}
