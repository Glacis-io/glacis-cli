//! # Glacis Core
//!
//! Core library for the Glacis Chain of Custody system.
//!
//! This crate provides the foundational infrastructure for building
//! secure, auditable compliance management systems.
//!
//! ## Features
//!
//! - **Audit Logging** - Structured audit logs for CTE operations
//! - **Request Tracing** - Correlation ID propagation for distributed tracing
//! - **Metrics Collection** - Latency percentiles, error rates, cache hit ratios
//! - **Key Management** - Versioned key rotation with grace periods
//! - **Rate Limiting** - Per-organization rate limiting with token bucket
//! - **Caching** - LRU cache with TTL and cache-aside pattern
//! - **Retry Queue** - Webhook retry with exponential backoff
//! - **Bulk Import** - CSV, JSON, JSONL, and OSCAL format support
//! - **Property Testing** - Coverage invariant testing infrastructure
//!
//! ## Example
//!
//! ```rust,no_run
//! use glacis_core::audit::{AuditEntry, CTEOperationType, InMemoryAuditStore, AuditStore};
//! use glacis_core::metrics::GlacisMetrics;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Set up audit logging
//!     let audit_store = InMemoryAuditStore::new(10000);
//!
//!     // Log a coverage computation
//!     let entry = AuditEntry::new(
//!         CTEOperationType::CoverageComputation,
//!         "Computing SOC2 coverage for policy POL-123"
//!     )
//!     .with_org_id("org-456")
//!     .with_resource("policy", "POL-123");
//!
//!     audit_store.log(entry).await.unwrap();
//!
//!     // Set up metrics
//!     let metrics = GlacisMetrics::new();
//!     metrics.record_request(Duration::from_millis(50));
//! }
//! ```

#![forbid(unsafe_code)]
#![allow(missing_docs)] // TODO: Add comprehensive documentation
#![warn(rust_2018_idioms)]

pub mod audit;
pub mod cache;
pub mod import;
pub mod keys;
pub mod metrics;
pub mod rate_limit;
pub mod retry;
pub mod testing;
pub mod tracing;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::audit::{
        AuditEntry, AuditError, AuditFilter, AuditLevel, AuditStore, CTEOperationType,
        InMemoryAuditStore, OperationOutcome,
    };
    pub use crate::cache::{CacheAside, CacheBackend, CacheStats, LruCache, TypedCache};
    pub use crate::import::{
        CsvImporter, ImportConfig, ImportFormat, ImportRecord, JsonImporter, OscalImporter,
        RecordValidator,
    };
    pub use crate::keys::{
        generate_key_material, InMemoryKeyStore, KeyError, KeyManager, KeyStore, KeyType,
        RotationConfig, VersionedKey,
    };
    pub use crate::metrics::{Counter, Gauge, GlacisMetrics, Histogram, MetricsRegistry};
    pub use crate::rate_limit::{
        create_shared_limiter, RateLimitConfig, RateLimitError, RateLimitResult, RateLimiter,
    };
    pub use crate::retry::{
        InMemoryQueueStore, ProcessResult, QueueItem, QueueStore, RetryQueue, RetryQueueConfig,
        WebhookPayload,
    };
    pub use crate::testing::{CoverageScore, Policy, Requirement};
    pub use crate::tracing::{CorrelationId, RequestContext, Span, SpanCollector};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prelude_imports() {
        // Verify prelude exports compile
        use prelude::*;

        let _score = CoverageScore::new(0.5);
        let _policy = Policy::new("TEST");
        let _req = Requirement::new("REQ-1", 0.8);
    }
}
