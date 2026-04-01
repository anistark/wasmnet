use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::debug;

/// Pool of idle TCP connections keyed by target address (`host:port`).
pub struct ConnectionPool {
    idle: Mutex<HashMap<String, VecDeque<PoolEntry>>>,
    max_idle_time: Duration,
    max_per_key: usize,
}

struct PoolEntry {
    stream: TcpStream,
    idle_since: Instant,
}

impl ConnectionPool {
    pub fn new(max_idle_secs: u64, max_per_key: usize) -> Self {
        Self {
            idle: Mutex::new(HashMap::new()),
            max_idle_time: Duration::from_secs(max_idle_secs),
            max_per_key,
        }
    }

    pub async fn get(&self, target: &str) -> Option<TcpStream> {
        let mut map = self.idle.lock().await;
        let entries = map.get_mut(target)?;

        while let Some(entry) = entries.pop_front() {
            if entry.idle_since.elapsed() < self.max_idle_time {
                if entries.is_empty() {
                    map.remove(target);
                }
                debug!("pool hit for {target}");
                return Some(entry.stream);
            }
        }
        map.remove(target);
        None
    }

    pub async fn put(&self, target: String, stream: TcpStream) {
        let mut map = self.idle.lock().await;
        let entries = map.entry(target).or_default();
        if entries.len() < self.max_per_key {
            entries.push_back(PoolEntry {
                stream,
                idle_since: Instant::now(),
            });
        }
    }

    pub async fn warm(&self, target: &str, count: usize) -> usize {
        let mut success = 0;
        for _ in 0..count {
            match TcpStream::connect(target).await {
                Ok(stream) => {
                    self.put(target.to_string(), stream).await;
                    success += 1;
                }
                Err(_) => break,
            }
        }
        debug!("warmed {success}/{count} connections to {target}");
        success
    }

    pub async fn cleanup(&self) {
        let mut map = self.idle.lock().await;
        map.retain(|_, entries| {
            entries.retain(|e| e.idle_since.elapsed() < self.max_idle_time);
            !entries.is_empty()
        });
    }

    pub fn start_cleanup_task(self: &Arc<Self>) {
        let weak: Weak<Self> = Arc::downgrade(self);
        let interval = (self.max_idle_time / 2).max(Duration::from_secs(1));
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                match weak.upgrade() {
                    Some(pool) => pool.cleanup().await,
                    None => break,
                }
            }
        });
    }

    pub async fn size(&self) -> usize {
        let map = self.idle.lock().await;
        map.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn put_and_get() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let target = addr.to_string();

        let pool = ConnectionPool::new(30, 4);

        let connect_handle = tokio::spawn({
            let target = target.clone();
            async move { TcpStream::connect(&target).await.unwrap() }
        });
        let (_server_stream, _) = listener.accept().await.unwrap();
        let client_stream = connect_handle.await.unwrap();

        pool.put(target.clone(), client_stream).await;
        assert_eq!(pool.size().await, 1);

        let got = pool.get(&target).await;
        assert!(got.is_some());
        assert_eq!(pool.size().await, 0);
    }

    #[tokio::test]
    async fn expired_entries_pruned() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let target = addr.to_string();

        let pool = ConnectionPool::new(0, 4); // 0-second idle = immediate expiry

        let connect_handle = tokio::spawn({
            let target = target.clone();
            async move { TcpStream::connect(&target).await.unwrap() }
        });
        let (_server_stream, _) = listener.accept().await.unwrap();
        let client_stream = connect_handle.await.unwrap();

        pool.put(target.clone(), client_stream).await;
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(pool.get(&target).await.is_none());
    }
}
