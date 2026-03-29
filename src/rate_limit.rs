use std::time::Instant;
use tokio::sync::Mutex;

/// Token-bucket rate limiter that throttles bandwidth by sleeping when tokens are exhausted.
pub struct RateLimiter {
    inner: Option<Mutex<Bucket>>,
}

struct Bucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(max_bandwidth_mbps: u32) -> Self {
        if max_bandwidth_mbps == 0 {
            return Self { inner: None };
        }
        let bps = f64::from(max_bandwidth_mbps) * 1_000_000.0 / 8.0;
        Self {
            inner: Some(Mutex::new(Bucket {
                tokens: bps,
                capacity: bps,
                refill_rate: bps,
                last_refill: Instant::now(),
            })),
        }
    }

    pub fn unlimited() -> Self {
        Self { inner: None }
    }

    pub async fn consume(&self, bytes: usize) {
        let bucket_mu = match &self.inner {
            Some(b) => b,
            None => return,
        };

        let mut b = bucket_mu.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(b.last_refill).as_secs_f64();
        b.last_refill = now;
        b.tokens = (b.tokens + elapsed * b.refill_rate).min(b.capacity);

        let needed = bytes as f64;
        if b.tokens >= needed {
            b.tokens -= needed;
        } else {
            let deficit = needed - b.tokens;
            b.tokens = 0.0;
            let wait = std::time::Duration::from_secs_f64(deficit / b.refill_rate);
            drop(b);
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unlimited_does_not_block() {
        let rl = RateLimiter::unlimited();
        rl.consume(1_000_000).await;
    }

    #[tokio::test]
    async fn small_transfer_within_budget() {
        let rl = RateLimiter::new(10); // 10 Mbps ≈ 1.25 MB/s
        let start = Instant::now();
        rl.consume(1024).await;
        assert!(start.elapsed().as_millis() < 50);
    }

    #[tokio::test]
    async fn over_budget_throttles() {
        let rl = RateLimiter::new(1); // 1 Mbps ≈ 125 KB/s capacity
        rl.consume(125_000).await; // drain the bucket
        let start = Instant::now();
        rl.consume(125_000).await; // should sleep ~1s
        assert!(start.elapsed().as_millis() >= 500);
    }
}
