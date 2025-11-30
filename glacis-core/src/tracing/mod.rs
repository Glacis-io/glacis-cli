//! Request Tracing with Correlation IDs
//!
//! Provides distributed tracing capabilities with:
//! - Correlation ID (x-request-id) propagation
//! - Span tracking for operation timing
//! - Context propagation across async boundaries

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use uuid::Uuid;

/// A correlation ID for request tracing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId(Uuid);

impl CorrelationId {
    /// Create a new random correlation ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a correlation ID from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parse a correlation ID from a string (x-request-id header)
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Get the string representation for headers
    pub fn to_header_value(&self) -> String {
        self.0.to_string()
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for CorrelationId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// A span representing a traced operation
#[derive(Debug, Clone)]
pub struct Span {
    /// Unique ID for this span
    pub span_id: Uuid,
    /// Parent span ID if this is a child span
    pub parent_span_id: Option<Uuid>,
    /// Correlation ID linking all spans in a request
    pub correlation_id: CorrelationId,
    /// Name of the operation
    pub operation: String,
    /// When the span started
    pub start_time: Instant,
    /// When the span ended (None if still active)
    pub end_time: Option<Instant>,
    /// Tags/attributes for the span
    pub tags: Vec<(String, String)>,
    /// Whether the operation succeeded
    pub success: Option<bool>,
}

impl Span {
    /// Create a new span with the given operation name
    pub fn new(operation: impl Into<String>, correlation_id: CorrelationId) -> Self {
        Self {
            span_id: Uuid::new_v4(),
            parent_span_id: None,
            correlation_id,
            operation: operation.into(),
            start_time: Instant::now(),
            end_time: None,
            tags: Vec::new(),
            success: None,
        }
    }

    /// Create a child span
    pub fn child(&self, operation: impl Into<String>) -> Self {
        Self {
            span_id: Uuid::new_v4(),
            parent_span_id: Some(self.span_id),
            correlation_id: self.correlation_id,
            operation: operation.into(),
            start_time: Instant::now(),
            end_time: None,
            tags: Vec::new(),
            success: None,
        }
    }

    /// Add a tag to the span
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.push((key.into(), value.into()));
        self
    }

    /// Mark the span as finished
    pub fn finish(&mut self) {
        self.end_time = Some(Instant::now());
    }

    /// Mark the span as finished with success status
    pub fn finish_ok(&mut self) {
        self.end_time = Some(Instant::now());
        self.success = Some(true);
    }

    /// Mark the span as finished with failure status
    pub fn finish_err(&mut self) {
        self.end_time = Some(Instant::now());
        self.success = Some(false);
    }

    /// Get the duration of the span (if finished)
    pub fn duration(&self) -> Option<Duration> {
        self.end_time.map(|end| end.duration_since(self.start_time))
    }

    /// Get the elapsed time (works even if not finished)
    pub fn elapsed(&self) -> Duration {
        self.end_time
            .unwrap_or_else(Instant::now)
            .duration_since(self.start_time)
    }
}

/// Request context that propagates through the call stack
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// The correlation ID for this request
    pub correlation_id: CorrelationId,
    /// The current span
    pub current_span: Option<Arc<RwLock<Span>>>,
    /// Organization ID if authenticated
    pub org_id: Option<String>,
    /// User ID if authenticated
    pub user_id: Option<String>,
    /// Additional context attributes
    pub attributes: Vec<(String, String)>,
}

impl RequestContext {
    /// Create a new request context with a new correlation ID
    pub fn new() -> Self {
        Self {
            correlation_id: CorrelationId::new(),
            current_span: None,
            org_id: None,
            user_id: None,
            attributes: Vec::new(),
        }
    }

    /// Create a request context from an incoming x-request-id header
    pub fn from_header(header_value: &str) -> Self {
        let correlation_id = CorrelationId::parse(header_value)
            .unwrap_or_else(|_| CorrelationId::new());
        Self {
            correlation_id,
            current_span: None,
            org_id: None,
            user_id: None,
            attributes: Vec::new(),
        }
    }

    /// Set the organization context
    pub fn with_org(mut self, org_id: impl Into<String>) -> Self {
        self.org_id = Some(org_id.into());
        self
    }

    /// Set the user context
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Add an attribute to the context
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    /// Start a new span in this context
    pub fn start_span(&mut self, operation: impl Into<String>) -> Arc<RwLock<Span>> {
        let span = if let Some(ref parent) = self.current_span {
            // This is a blocking read, but we're just getting the span to create a child
            let parent_guard = futures::executor::block_on(parent.read());
            parent_guard.child(operation)
        } else {
            Span::new(operation, self.correlation_id)
        };

        let span = Arc::new(RwLock::new(span));
        self.current_span = Some(span.clone());
        span
    }

    /// Get the x-request-id header value for outgoing requests
    pub fn request_id_header(&self) -> (&'static str, String) {
        ("x-request-id", self.correlation_id.to_header_value())
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

/// A guard that finishes a span when dropped
pub struct SpanGuard {
    span: Arc<RwLock<Span>>,
    success: bool,
}

impl SpanGuard {
    pub fn new(span: Arc<RwLock<Span>>) -> Self {
        Self { span, success: true }
    }

    /// Mark the span as failed
    pub fn set_error(&mut self) {
        self.success = false;
    }

    /// Mark the span as successful
    pub fn set_ok(&mut self) {
        self.success = true;
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        // Use tokio's block_in_place if available, otherwise best-effort
        let span = self.span.clone();
        let success = self.success;
        tokio::spawn(async move {
            let mut guard = span.write().await;
            if success {
                guard.finish_ok();
            } else {
                guard.finish_err();
            }
        });
    }
}

/// Extension trait for futures to add tracing
pub trait TracingExt: Sized {
    /// Wrap this future in a span
    fn with_span(self, ctx: &mut RequestContext, operation: impl Into<String>) -> TracedFuture<Self>;
}

impl<F: Future> TracingExt for F {
    fn with_span(self, ctx: &mut RequestContext, operation: impl Into<String>) -> TracedFuture<Self> {
        let span = ctx.start_span(operation);
        TracedFuture {
            inner: self,
            span,
            finished: false,
        }
    }
}

/// A future wrapper that records timing in a span
#[pin_project::pin_project]
pub struct TracedFuture<F> {
    #[pin]
    inner: F,
    span: Arc<RwLock<Span>>,
    finished: bool,
}

impl<F: Future> Future for TracedFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = this.inner.poll(cx);

        if result.is_ready() && !*this.finished {
            *this.finished = true;
            // Mark span as finished (best effort, non-blocking)
            let span = this.span.clone();
            tokio::spawn(async move {
                let mut guard = span.write().await;
                guard.finish_ok();
            });
        }

        result
    }
}

/// Trait for collectors that receive span data
#[async_trait::async_trait]
pub trait SpanCollector: Send + Sync {
    /// Called when a span is finished
    async fn collect(&self, span: &Span);
}

/// A no-op collector for testing
pub struct NoopCollector;

#[async_trait::async_trait]
impl SpanCollector for NoopCollector {
    async fn collect(&self, _span: &Span) {}
}

/// A collector that logs spans
pub struct LoggingCollector;

#[async_trait::async_trait]
impl SpanCollector for LoggingCollector {
    async fn collect(&self, span: &Span) {
        let duration_ms = span.elapsed().as_millis();
        let status = match span.success {
            Some(true) => "OK",
            Some(false) => "ERR",
            None => "???",
        };

        tracing::info!(
            correlation_id = %span.correlation_id,
            span_id = %span.span_id,
            operation = %span.operation,
            duration_ms = %duration_ms,
            status = %status,
            "Span completed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correlation_id_creation() {
        let id1 = CorrelationId::new();
        let id2 = CorrelationId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_correlation_id_parsing() {
        let id = CorrelationId::new();
        let header = id.to_header_value();
        let parsed = CorrelationId::parse(&header).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_span_hierarchy() {
        let cid = CorrelationId::new();
        let parent = Span::new("parent_op", cid);
        let child = parent.child("child_op");

        assert_eq!(child.parent_span_id, Some(parent.span_id));
        assert_eq!(child.correlation_id, parent.correlation_id);
    }

    #[test]
    fn test_request_context() {
        let ctx = RequestContext::new()
            .with_org("org-123")
            .with_user("user-456")
            .with_attribute("env", "production");

        assert_eq!(ctx.org_id, Some("org-123".to_string()));
        assert_eq!(ctx.user_id, Some("user-456".to_string()));
        assert!(ctx.attributes.contains(&("env".to_string(), "production".to_string())));
    }

    #[test]
    fn test_span_duration() {
        let cid = CorrelationId::new();
        let mut span = Span::new("test_op", cid);
        std::thread::sleep(std::time::Duration::from_millis(10));
        span.finish();

        let duration = span.duration().unwrap();
        assert!(duration >= std::time::Duration::from_millis(10));
    }
}
