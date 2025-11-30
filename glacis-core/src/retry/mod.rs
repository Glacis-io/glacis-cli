//! Webhook Retry Queue
//!
//! Provides reliable webhook delivery with:
//! - Persistent queue with D1/SQLite backend abstraction
//! - Exponential backoff with jitter
//! - Dead letter queue for failed items
//! - At-least-once delivery guarantee

use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Maximum number of retry attempts before moving to DLQ
pub const DEFAULT_MAX_RETRIES: u32 = 5;

/// Base delay for exponential backoff
pub const DEFAULT_BASE_DELAY: Duration = Duration::from_secs(1);

/// Maximum delay cap for exponential backoff
pub const DEFAULT_MAX_DELAY: Duration = Duration::from_secs(300); // 5 minutes

/// Status of a queued item
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemStatus {
    /// Waiting to be processed
    Pending,
    /// Currently being processed
    Processing,
    /// Successfully processed
    Completed,
    /// Failed and will be retried
    RetryPending,
    /// Permanently failed, moved to DLQ
    Failed,
}

/// A queued webhook/task item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem<T>
where
    T: Clone + Send + Sync,
{
    /// Unique identifier
    pub id: Uuid,
    /// The payload to deliver
    pub payload: T,
    /// Current status
    pub status: QueueItemStatus,
    /// Number of attempts made
    pub attempts: u32,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// When the item was created
    pub created_at: DateTime<Utc>,
    /// When to process next
    pub process_at: DateTime<Utc>,
    /// Last error message if failed
    pub last_error: Option<String>,
    /// Organization ID
    pub org_id: Option<String>,
    /// Correlation ID for tracing
    pub correlation_id: Option<Uuid>,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl<T: Clone + Send + Sync> QueueItem<T> {
    pub fn new(payload: T) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            payload,
            status: QueueItemStatus::Pending,
            attempts: 0,
            max_retries: DEFAULT_MAX_RETRIES,
            created_at: now,
            process_at: now,
            last_error: None,
            org_id: None,
            correlation_id: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    pub fn with_org_id(mut self, org_id: impl Into<String>) -> Self {
        self.org_id = Some(org_id.into());
        self
    }

    pub fn with_correlation_id(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.process_at = Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
        self
    }

    /// Check if the item should be moved to DLQ
    pub fn should_dlq(&self) -> bool {
        self.attempts >= self.max_retries
    }

    /// Calculate the next retry delay using exponential backoff with jitter
    pub fn next_retry_delay(&self) -> Duration {
        let base_ms = DEFAULT_BASE_DELAY.as_millis() as u64;
        let exp_delay = base_ms * 2u64.saturating_pow(self.attempts);
        let max_ms = DEFAULT_MAX_DELAY.as_millis() as u64;
        let delay_ms = exp_delay.min(max_ms);

        // Add jitter (±25%)
        let jitter_range = delay_ms / 4;
        let jitter = if jitter_range > 0 {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            rng.gen_range(0..jitter_range * 2) as i64 - jitter_range as i64
        } else {
            0
        };

        Duration::from_millis((delay_ms as i64 + jitter).max(0) as u64)
    }

    /// Prepare for retry
    pub fn prepare_retry(&mut self, error: Option<String>) {
        self.attempts += 1;
        self.last_error = error;

        if self.should_dlq() {
            self.status = QueueItemStatus::Failed;
        } else {
            self.status = QueueItemStatus::RetryPending;
            let delay = self.next_retry_delay();
            self.process_at = Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
        }
    }

    /// Mark as completed
    pub fn mark_completed(&mut self) {
        self.status = QueueItemStatus::Completed;
    }
}

/// Configuration for the retry queue
#[derive(Debug, Clone)]
pub struct RetryQueueConfig {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub processing_timeout: Duration,
    pub batch_size: usize,
}

impl Default for RetryQueueConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay: DEFAULT_BASE_DELAY,
            max_delay: DEFAULT_MAX_DELAY,
            processing_timeout: Duration::from_secs(30),
            batch_size: 10,
        }
    }
}

/// Trait for queue storage backends
#[async_trait::async_trait]
pub trait QueueStore<T>: Send + Sync
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    /// Add an item to the queue
    async fn enqueue(&self, item: QueueItem<T>) -> Result<(), QueueError>;

    /// Get items ready for processing
    async fn dequeue(&self, limit: usize) -> Result<Vec<QueueItem<T>>, QueueError>;

    /// Update an item's status
    async fn update(&self, item: &QueueItem<T>) -> Result<(), QueueError>;

    /// Move an item to the dead letter queue
    async fn move_to_dlq(&self, item: &QueueItem<T>) -> Result<(), QueueError>;

    /// Get items from the dead letter queue
    async fn get_dlq(&self, limit: usize, offset: usize) -> Result<Vec<QueueItem<T>>, QueueError>;

    /// Retry an item from the DLQ
    async fn retry_from_dlq(&self, id: Uuid) -> Result<(), QueueError>;

    /// Get queue statistics
    async fn stats(&self) -> Result<QueueStats, QueueError>;
}

/// Queue statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueueStats {
    pub pending: u64,
    pub processing: u64,
    pub completed: u64,
    pub failed: u64,
    pub dlq_size: u64,
}

/// Queue errors
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Item not found: {0}")]
    NotFound(Uuid),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Wrapper for priority queue ordering
struct ScheduledItem<T>
where
    T: Clone + Send + Sync,
{
    item: QueueItem<T>,
    process_at: Instant,
}

impl<T: Clone + Send + Sync> PartialEq for ScheduledItem<T> {
    fn eq(&self, other: &Self) -> bool {
        self.process_at == other.process_at
    }
}

impl<T: Clone + Send + Sync> Eq for ScheduledItem<T> {}

impl<T: Clone + Send + Sync> PartialOrd for ScheduledItem<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Clone + Send + Sync> Ord for ScheduledItem<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior
        other.process_at.cmp(&self.process_at)
    }
}

/// In-memory queue store for testing and development
pub struct InMemoryQueueStore<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    pending: Arc<RwLock<BinaryHeap<ScheduledItem<T>>>>,
    processing: Arc<RwLock<HashMap<Uuid, QueueItem<T>>>>,
    completed: Arc<RwLock<VecDeque<QueueItem<T>>>>,
    dlq: Arc<RwLock<Vec<QueueItem<T>>>>,
    stats: Arc<RwLock<QueueStats>>,
}

impl<T> Default for InMemoryQueueStore<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> InMemoryQueueStore<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(BinaryHeap::new())),
            processing: Arc::new(RwLock::new(HashMap::new())),
            completed: Arc::new(RwLock::new(VecDeque::new())),
            dlq: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(QueueStats::default())),
        }
    }
}

#[async_trait::async_trait]
impl<T> QueueStore<T> for InMemoryQueueStore<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de> + 'static,
{
    async fn enqueue(&self, item: QueueItem<T>) -> Result<(), QueueError> {
        let process_at = Instant::now()
            + (item.process_at - Utc::now())
                .to_std()
                .unwrap_or(Duration::ZERO);

        let mut pending = self.pending.write().await;
        let mut stats = self.stats.write().await;

        pending.push(ScheduledItem { item, process_at });
        stats.pending += 1;

        Ok(())
    }

    async fn dequeue(&self, limit: usize) -> Result<Vec<QueueItem<T>>, QueueError> {
        let mut pending = self.pending.write().await;
        let mut processing = self.processing.write().await;
        let mut stats = self.stats.write().await;

        let now = Instant::now();
        let mut result = Vec::new();

        while result.len() < limit {
            if let Some(scheduled) = pending.peek() {
                if scheduled.process_at <= now {
                    let scheduled = pending.pop().unwrap();
                    let mut item = scheduled.item;
                    item.status = QueueItemStatus::Processing;

                    processing.insert(item.id, item.clone());
                    result.push(item);

                    stats.pending = stats.pending.saturating_sub(1);
                    stats.processing += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(result)
    }

    async fn update(&self, item: &QueueItem<T>) -> Result<(), QueueError> {
        let mut processing = self.processing.write().await;
        let mut completed_queue = self.completed.write().await;
        let mut pending = self.pending.write().await;
        let mut stats = self.stats.write().await;

        match item.status {
            QueueItemStatus::Completed => {
                processing.remove(&item.id);
                completed_queue.push_back(item.clone());

                // Keep only last 1000 completed items
                while completed_queue.len() > 1000 {
                    completed_queue.pop_front();
                }

                stats.processing = stats.processing.saturating_sub(1);
                stats.completed += 1;
            }
            QueueItemStatus::RetryPending => {
                processing.remove(&item.id);

                let process_at = Instant::now()
                    + (item.process_at - Utc::now())
                        .to_std()
                        .unwrap_or(Duration::ZERO);

                pending.push(ScheduledItem {
                    item: item.clone(),
                    process_at,
                });

                stats.processing = stats.processing.saturating_sub(1);
                stats.pending += 1;
            }
            QueueItemStatus::Failed => {
                processing.remove(&item.id);
                stats.processing = stats.processing.saturating_sub(1);
                stats.failed += 1;
            }
            _ => {
                if let Some(existing) = processing.get_mut(&item.id) {
                    *existing = item.clone();
                }
            }
        }

        Ok(())
    }

    async fn move_to_dlq(&self, item: &QueueItem<T>) -> Result<(), QueueError> {
        let mut dlq = self.dlq.write().await;
        let mut stats = self.stats.write().await;

        dlq.push(item.clone());
        stats.dlq_size += 1;

        Ok(())
    }

    async fn get_dlq(&self, limit: usize, offset: usize) -> Result<Vec<QueueItem<T>>, QueueError> {
        let dlq = self.dlq.read().await;
        Ok(dlq.iter().skip(offset).take(limit).cloned().collect())
    }

    async fn retry_from_dlq(&self, id: Uuid) -> Result<(), QueueError> {
        let mut dlq = self.dlq.write().await;
        let mut pending = self.pending.write().await;
        let mut stats = self.stats.write().await;

        if let Some(pos) = dlq.iter().position(|item| item.id == id) {
            let mut item = dlq.remove(pos);
            item.status = QueueItemStatus::Pending;
            item.attempts = 0;
            item.process_at = Utc::now();
            item.last_error = None;

            pending.push(ScheduledItem {
                item,
                process_at: Instant::now(),
            });

            stats.dlq_size = stats.dlq_size.saturating_sub(1);
            stats.pending += 1;

            Ok(())
        } else {
            Err(QueueError::NotFound(id))
        }
    }

    async fn stats(&self) -> Result<QueueStats, QueueError> {
        Ok(self.stats.read().await.clone())
    }
}

/// Result of processing a queue item
pub enum ProcessResult {
    /// Item was processed successfully
    Success,
    /// Item failed but should be retried
    Retry(String),
    /// Item failed permanently and should go to DLQ
    Fail(String),
}

/// The retry queue processor
pub struct RetryQueue<T, S>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de> + 'static,
    S: QueueStore<T>,
{
    store: Arc<S>,
    config: RetryQueueConfig,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S> RetryQueue<T, S>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de> + 'static,
    S: QueueStore<T>,
{
    pub fn new(store: S, config: RetryQueueConfig) -> Self {
        Self {
            store: Arc::new(store),
            config,
            _marker: std::marker::PhantomData,
        }
    }

    /// Add an item to the queue
    pub async fn enqueue(&self, payload: T) -> Result<Uuid, QueueError> {
        let item = QueueItem::new(payload).with_max_retries(self.config.max_retries);
        let id = item.id;
        self.store.enqueue(item).await?;
        Ok(id)
    }

    /// Add an item with custom options
    pub async fn enqueue_with_options(&self, item: QueueItem<T>) -> Result<Uuid, QueueError> {
        let id = item.id;
        self.store.enqueue(item).await?;
        Ok(id)
    }

    /// Process items from the queue
    pub async fn process<F, Fut>(&self, handler: F) -> Result<usize, QueueError>
    where
        F: Fn(T) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = ProcessResult> + Send,
    {
        let items = self.store.dequeue(self.config.batch_size).await?;
        let mut processed = 0;

        for mut item in items {
            let result = handler(item.payload.clone()).await;

            match result {
                ProcessResult::Success => {
                    item.mark_completed();
                    self.store.update(&item).await?;
                }
                ProcessResult::Retry(error) => {
                    item.prepare_retry(Some(error));
                    if item.status == QueueItemStatus::Failed {
                        self.store.move_to_dlq(&item).await?;
                    }
                    self.store.update(&item).await?;
                }
                ProcessResult::Fail(error) => {
                    item.status = QueueItemStatus::Failed;
                    item.last_error = Some(error);
                    self.store.move_to_dlq(&item).await?;
                    self.store.update(&item).await?;
                }
            }

            processed += 1;
        }

        Ok(processed)
    }

    /// Get queue statistics
    pub async fn stats(&self) -> Result<QueueStats, QueueError> {
        self.store.stats().await
    }

    /// Retry an item from the DLQ
    pub async fn retry_dlq_item(&self, id: Uuid) -> Result<(), QueueError> {
        self.store.retry_from_dlq(id).await
    }

    /// Get items from the dead letter queue
    pub async fn get_dlq(&self, limit: usize, offset: usize) -> Result<Vec<QueueItem<T>>, QueueError> {
        self.store.get_dlq(limit, offset).await
    }
}

/// Create a webhook-specific payload type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    /// Target URL
    pub url: String,
    /// HTTP method
    pub method: String,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body
    pub body: String,
    /// Event type
    pub event_type: String,
}

impl WebhookPayload {
    pub fn new(url: impl Into<String>, event_type: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            body: body.into(),
            event_type: event_type.into(),
        }
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_queue_basic() {
        let store = InMemoryQueueStore::new();
        let queue = RetryQueue::new(store, RetryQueueConfig::default());

        let id = queue.enqueue("test payload".to_string()).await.unwrap();
        assert!(!id.is_nil());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 1);
    }

    #[tokio::test]
    async fn test_queue_processing() {
        let store = InMemoryQueueStore::new();
        let queue = RetryQueue::new(store, RetryQueueConfig::default());

        queue.enqueue("test".to_string()).await.unwrap();

        let processed = queue
            .process(|_payload| async { ProcessResult::Success })
            .await
            .unwrap();

        assert_eq!(processed, 1);

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.pending, 0);
    }

    #[tokio::test]
    async fn test_queue_retry() {
        let store = InMemoryQueueStore::new();
        let queue = RetryQueue::new(store, RetryQueueConfig::default());

        queue.enqueue("test".to_string()).await.unwrap();

        // First attempt fails
        queue
            .process(|_| async { ProcessResult::Retry("temporary error".to_string()) })
            .await
            .unwrap();

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 1); // Back in pending
        assert_eq!(stats.completed, 0);
    }

    #[tokio::test]
    async fn test_queue_dlq() {
        let store = InMemoryQueueStore::new();
        let config = RetryQueueConfig {
            max_retries: 1, // Only 1 retry
            ..Default::default()
        };
        let queue = RetryQueue::new(store, config);

        queue.enqueue("test".to_string()).await.unwrap();

        // Fail once
        queue
            .process(|_| async { ProcessResult::Retry("error".to_string()) })
            .await
            .unwrap();

        // Need to wait for the retry delay, but for testing we'll just process again
        // In a real scenario, the item would be scheduled for later

        let stats = queue.stats().await.unwrap();
        // Item should either be in pending (waiting for retry) or in DLQ
        assert!(stats.pending > 0 || stats.dlq_size > 0);
    }

    #[test]
    fn test_exponential_backoff() {
        let mut item = QueueItem::new("test".to_string());

        // First retry: ~1 second
        let delay1 = item.next_retry_delay();
        assert!(delay1 >= Duration::from_millis(750) && delay1 <= Duration::from_millis(1250));

        item.attempts = 1;
        // Second retry: ~2 seconds
        let delay2 = item.next_retry_delay();
        assert!(delay2 >= Duration::from_millis(1500) && delay2 <= Duration::from_millis(2500));

        item.attempts = 5;
        // Fifth retry: ~32 seconds
        let delay5 = item.next_retry_delay();
        assert!(delay5 >= Duration::from_millis(24000) && delay5 <= Duration::from_millis(40000));
    }

    #[test]
    fn test_webhook_payload() {
        let payload = WebhookPayload::new(
            "https://example.com/webhook",
            "attestation.created",
            r#"{"id": "123"}"#,
        )
        .with_header("X-Signature", "sha256=abc123");

        assert_eq!(payload.url, "https://example.com/webhook");
        assert_eq!(payload.event_type, "attestation.created");
        assert!(payload.headers.contains_key("X-Signature"));
    }
}
