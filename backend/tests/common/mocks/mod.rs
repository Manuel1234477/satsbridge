//! Test doubles for the collection and payout providers. They live in the test harness, not the
//! shipped crate: production registers only real providers.
pub mod collection;
pub mod mobile_money;
pub mod ngn;
pub mod tando;
