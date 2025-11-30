//! Caching Layer Abstraction
//!
//! Provides a unified caching interface with:
//! - In-memory cache with TTL
//! - LRU eviction policy
//! - Cache-aside pattern helpers
//! - Backend abstraction for KV stores

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{de::DeserializeOwned, Serialize};

/// A cached value with metadata
#[derive(Debug, Clone)]
pub struct CachedValue<T> {
    /// The actual value
    pub value: T,
    /// When the value was cached
    pub cached_at: Instant,
    /// When the value expires
    pub expires_at: Option<Instant>,
    /// Cache generation (for invalidation)
    pub generation: u64,
}

impl<T> CachedValue<T> {
    pub fn new(value: T, ttl: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            value,
            cached_at: now,
            expires_at: ttl.map(|d| now + d),
            generation: 0,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at.map(|e| Instant::now() > e).unwrap_or(false)
    }

    pub fn age(&self) -> Duration {
        self.cached_at.elapsed()
    }

    pub fn ttl_remaining(&self) -> Option<Duration> {
        self.expires_at.and_then(|e| {
            let now = Instant::now();
            if now < e {
                Some(e - now)
            } else {
                None
            }
        })
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size: usize,
}

impl CacheStats {
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Trait for cache backends
#[async_trait::async_trait]
pub trait CacheBackend: Send + Sync {
    type Key: Clone + Eq + Hash + Send + Sync;
    type Value: Clone + Send + Sync;

    /// Get a value from the cache
    async fn get(&self, key: &Self::Key) -> Option<CachedValue<Self::Value>>;

    /// Set a value in the cache
    async fn set(&self, key: Self::Key, value: Self::Value, ttl: Option<Duration>);

    /// Delete a value from the cache
    async fn delete(&self, key: &Self::Key) -> bool;

    /// Check if a key exists
    async fn exists(&self, key: &Self::Key) -> bool;

    /// Clear all entries
    async fn clear(&self);

    /// Get cache statistics
    async fn stats(&self) -> CacheStats;
}

/// LRU node for tracking access order
struct LruNode<K> {
    key: K,
    prev: Option<usize>,
    next: Option<usize>,
}

/// In-memory LRU cache
pub struct LruCache<K, V>
where
    K: Clone + Eq + Hash + Send + Sync,
    V: Clone + Send + Sync,
{
    entries: RwLock<HashMap<K, (CachedValue<V>, usize)>>,
    lru_list: RwLock<Vec<LruNode<K>>>,
    head: RwLock<Option<usize>>,
    tail: RwLock<Option<usize>>,
    capacity: usize,
    default_ttl: Option<Duration>,
    stats: RwLock<CacheStats>,
}

impl<K, V> LruCache<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + Debug,
    V: Clone + Send + Sync,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            lru_list: RwLock::new(Vec::with_capacity(capacity)),
            head: RwLock::new(None),
            tail: RwLock::new(None),
            capacity,
            default_ttl: None,
            stats: RwLock::new(CacheStats::default()),
        }
    }

    pub fn with_default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = Some(ttl);
        self
    }

    fn move_to_front(&self, idx: usize) {
        let mut list = self.lru_list.write();
        let mut head = self.head.write();
        let mut tail = self.tail.write();

        // Already at front
        if *head == Some(idx) {
            return;
        }

        // Remove from current position
        if let Some(prev) = list[idx].prev {
            list[prev].next = list[idx].next;
        }
        if let Some(next) = list[idx].next {
            list[next].prev = list[idx].prev;
        }

        // Update tail if needed
        if *tail == Some(idx) {
            *tail = list[idx].prev;
        }

        // Move to front
        list[idx].prev = None;
        list[idx].next = *head;

        if let Some(old_head) = *head {
            list[old_head].prev = Some(idx);
        }

        *head = Some(idx);

        if tail.is_none() {
            *tail = Some(idx);
        }
    }

    fn evict_lru(&self) {
        let mut entries = self.entries.write();
        let mut list = self.lru_list.write();
        let mut head = self.head.write();
        let mut tail = self.tail.write();
        let mut stats = self.stats.write();

        if let Some(tail_idx) = *tail {
            let key = list[tail_idx].key.clone();
            entries.remove(&key);

            *tail = list[tail_idx].prev;
            if let Some(new_tail) = *tail {
                list[new_tail].next = None;
            } else {
                *head = None;
            }

            stats.evictions += 1;
            stats.size = entries.len();
        }
    }
}

#[async_trait::async_trait]
impl<K, V> CacheBackend for LruCache<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + Debug + 'static,
    V: Clone + Send + Sync + 'static,
{
    type Key = K;
    type Value = V;

    async fn get(&self, key: &K) -> Option<CachedValue<V>> {
        // Check cache and get result without holding locks across await
        let check_result = {
            let entries = self.entries.read();
            let mut stats = self.stats.write();

            if let Some((cached, idx)) = entries.get(key) {
                if cached.is_expired() {
                    stats.misses += 1;
                    Some((None, true)) // Need to delete
                } else {
                    stats.hits += 1;
                    Some((Some((cached.clone(), *idx)), false))
                }
            } else {
                stats.misses += 1;
                None
            }
        };

        match check_result {
            Some((None, true)) => {
                // Expired - delete and return None
                self.delete(key).await;
                None
            }
            Some((Some((result, idx)), false)) => {
                // Valid hit - move to front
                self.move_to_front(idx);
                Some(result)
            }
            _ => None,
        }
    }

    async fn set(&self, key: K, value: V, ttl: Option<Duration>) {
        let ttl = ttl.or(self.default_ttl);
        let cached = CachedValue::new(value, ttl);

        // Check if we need to evict
        let needs_evict = {
            let entries = self.entries.read();
            !entries.contains_key(&key) && entries.len() >= self.capacity
        };

        if needs_evict {
            self.evict_lru();
        }

        let mut entries = self.entries.write();
        let mut list = self.lru_list.write();
        let mut head = self.head.write();
        let mut tail = self.tail.write();
        let mut stats = self.stats.write();

        let idx = if let Some((_, existing_idx)) = entries.get(&key) {
            let idx = *existing_idx;
            drop(entries);
            drop(list);
            drop(head);
            drop(tail);
            drop(stats);
            self.move_to_front(idx);
            let mut entries = self.entries.write();
            if let Some((ref mut c, _)) = entries.get_mut(&key) {
                *c = cached;
            }
            return;
        } else {
            let idx = list.len();
            list.push(LruNode {
                key: key.clone(),
                prev: None,
                next: *head,
            });

            if let Some(old_head) = *head {
                list[old_head].prev = Some(idx);
            }
            *head = Some(idx);

            if tail.is_none() {
                *tail = Some(idx);
            }

            idx
        };

        entries.insert(key, (cached, idx));
        stats.size = entries.len();
    }

    async fn delete(&self, key: &K) -> bool {
        let mut entries = self.entries.write();
        let mut stats = self.stats.write();

        if entries.remove(key).is_some() {
            stats.size = entries.len();
            true
        } else {
            false
        }
    }

    async fn exists(&self, key: &K) -> bool {
        let entries = self.entries.read();
        if let Some((cached, _)) = entries.get(key) {
            !cached.is_expired()
        } else {
            false
        }
    }

    async fn clear(&self) {
        let mut entries = self.entries.write();
        let mut list = self.lru_list.write();
        let mut head = self.head.write();
        let mut tail = self.tail.write();
        let mut stats = self.stats.write();

        entries.clear();
        list.clear();
        *head = None;
        *tail = None;
        stats.size = 0;
    }

    async fn stats(&self) -> CacheStats {
        self.stats.read().clone()
    }
}

/// Cache-aside pattern helper
pub struct CacheAside<C: CacheBackend> {
    cache: Arc<C>,
}

impl<C: CacheBackend> CacheAside<C> {
    pub fn new(cache: Arc<C>) -> Self {
        Self { cache }
    }

    /// Get a value, fetching from source if not cached
    pub async fn get_or_fetch<F, Fut>(
        &self,
        key: &C::Key,
        fetch: F,
        ttl: Option<Duration>,
    ) -> Result<C::Value, CacheError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<C::Value, CacheError>>,
    {
        // Try cache first
        if let Some(cached) = self.cache.get(key).await {
            return Ok(cached.value);
        }

        // Fetch from source
        let value = fetch().await?;

        // Cache the result
        self.cache.set(key.clone(), value.clone(), ttl).await;

        Ok(value)
    }

    /// Invalidate a cached value
    pub async fn invalidate(&self, key: &C::Key) {
        self.cache.delete(key).await;
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        self.cache.stats().await
    }
}

/// Cache errors
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Fetch failed: {0}")]
    FetchFailed(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Backend error: {0}")]
    Backend(String),
}

/// Typed cache wrapper for JSON-serializable values
pub struct TypedCache<K>
where
    K: Clone + Eq + Hash + Send + Sync + Debug + 'static,
{
    inner: Arc<LruCache<K, Vec<u8>>>,
}

impl<K> TypedCache<K>
where
    K: Clone + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(LruCache::new(capacity)),
        }
    }

    pub fn with_default_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Arc::new(LruCache::new(capacity).with_default_ttl(ttl)),
        }
    }

    pub async fn get<V: DeserializeOwned>(&self, key: &K) -> Option<V> {
        self.inner.get(key).await.and_then(|cached| {
            serde_json::from_slice(&cached.value).ok()
        })
    }

    pub async fn set<V: Serialize>(&self, key: K, value: &V, ttl: Option<Duration>) -> Result<(), CacheError> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| CacheError::Serialization(e.to_string()))?;
        self.inner.set(key, bytes, ttl).await;
        Ok(())
    }

    pub async fn delete(&self, key: &K) -> bool {
        self.inner.delete(key).await
    }

    pub async fn stats(&self) -> CacheStats {
        self.inner.stats().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[tokio::test]
    async fn test_lru_cache_basic() {
        let cache: LruCache<String, i32> = LruCache::new(3);

        cache.set("a".to_string(), 1, None).await;
        cache.set("b".to_string(), 2, None).await;
        cache.set("c".to_string(), 3, None).await;

        assert_eq!(cache.get(&"a".to_string()).await.map(|c| c.value), Some(1));
        assert_eq!(cache.get(&"b".to_string()).await.map(|c| c.value), Some(2));
        assert_eq!(cache.get(&"c".to_string()).await.map(|c| c.value), Some(3));
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let cache: LruCache<String, i32> = LruCache::new(2);

        cache.set("a".to_string(), 1, None).await;
        cache.set("b".to_string(), 2, None).await;

        // Access 'a' to make it more recent
        cache.get(&"a".to_string()).await;

        // Add 'c', should evict 'b' (least recently used)
        cache.set("c".to_string(), 3, None).await;

        assert!(cache.get(&"a".to_string()).await.is_some());
        assert!(cache.get(&"b".to_string()).await.is_none());
        assert!(cache.get(&"c".to_string()).await.is_some());
    }

    #[tokio::test]
    async fn test_cache_ttl() {
        let cache: LruCache<String, i32> = LruCache::new(10);

        cache.set("a".to_string(), 1, Some(Duration::from_millis(50))).await;

        assert!(cache.get(&"a".to_string()).await.is_some());

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(cache.get(&"a".to_string()).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache: LruCache<String, i32> = LruCache::new(10);

        cache.set("a".to_string(), 1, None).await;

        // Hit
        cache.get(&"a".to_string()).await;
        // Miss
        cache.get(&"b".to_string()).await;

        let stats = cache.stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_ratio() - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_cache_aside() {
        let cache = Arc::new(LruCache::new(10));
        let aside = CacheAside::new(cache);

        let value = aside
            .get_or_fetch(
                &"key".to_string(),
                || async { Ok(42) },
                None,
            )
            .await
            .unwrap();

        assert_eq!(value, 42);

        // Should come from cache now
        let stats_before = aside.stats().await;
        let value2 = aside
            .get_or_fetch(
                &"key".to_string(),
                || async { Ok(99) }, // Different value, but shouldn't be used
                None,
            )
            .await
            .unwrap();

        assert_eq!(value2, 42); // Should still be cached value

        let stats_after = aside.stats().await;
        assert_eq!(stats_after.hits, stats_before.hits + 1);
    }

    #[tokio::test]
    async fn test_typed_cache() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }

        let cache: TypedCache<String> = TypedCache::new(10);

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        cache.set("key".to_string(), &data, None).await.unwrap();

        let retrieved: Option<TestData> = cache.get(&"key".to_string()).await;
        assert_eq!(retrieved, Some(data));
    }
}
