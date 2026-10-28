pub mod admin;
pub mod lnurl;
pub mod payments;
pub mod quotes;
pub mod recipients;
pub mod recovery;
pub mod refunds;
pub mod settlement;
pub mod users;

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}
