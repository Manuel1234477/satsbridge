//! Fixed-window rate limiting, in memory (one process). Keys are user IDs or client IPs.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::config::RateLimitConfig;

pub struct RateLimiter {
    limit: u32,
    window: Duration,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    /// `limit` of 0 disables the limiter.
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Counts one request for `key`; false once the window's budget is spent.
    pub fn check(&self, key: &str) -> bool {
        if self.limit == 0 {
            return true;
        }
        let now = Instant::now();
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        if windows.len() > 10_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < self.window);
        }
        let entry = windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit {
            return false;
        }
        entry.1 += 1;
        true
    }
}

pub struct RateLimits {
    pub api: RateLimiter,
    pub signups: RateLimiter,
}

impl RateLimits {
    pub fn new(cfg: &RateLimitConfig) -> Self {
        Self {
            api: RateLimiter::new(cfg.per_minute, Duration::from_secs(60)),
            signups: RateLimiter::new(cfg.signups_per_hour, Duration::from_secs(3600)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_per_key() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check("a") && limiter.check("a"));
        assert!(!limiter.check("a"));
        assert!(limiter.check("b"));
        assert!(RateLimiter::new(0, Duration::from_secs(1)).check("x"));
    }
}
