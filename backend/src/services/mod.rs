pub mod payments;
pub mod quotes;
pub mod recipients;
pub mod settlement;

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}
