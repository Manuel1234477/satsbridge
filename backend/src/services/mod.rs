pub mod admin;
pub mod ledger;
pub mod payments;
pub mod quotes;
pub mod recipients;
pub mod recovery;
pub mod refunds;
pub mod users;

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}
