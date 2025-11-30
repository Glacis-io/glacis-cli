//! Integration Tests for Glacis Core
//!
//! These tests exercise the full functionality of core modules
//! in realistic scenarios.

use glacis_core::*;
use std::time::Duration;

mod audit_integration {
    use super::*;
    use audit::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_full_audit_workflow() {
        let store = InMemoryAuditStore::new(1000);

        // Simulate a coverage computation workflow
        let correlation_id = Uuid::new_v4();
        let org_id = "org-integration-test";

        // Log operation start
        let entry_start = AuditEntry::new(CTEOperationType::CoverageComputation, "Starting coverage computation")
            .with_correlation_id(correlation_id)
            .with_org_id(org_id)
            .with_resource("policy", "POL-123")
            .with_metadata("framework", "SOC2");

        store.log(entry_start).await.unwrap();

        // Log operation completion
        let entry_complete = AuditEntry::new(CTEOperationType::CoverageComputation, "Coverage computation complete")
            .with_correlation_id(correlation_id)
            .with_org_id(org_id)
            .with_resource("policy", "POL-123")
            .with_outcome(OperationOutcome::Success)
            .with_metadata("coverage_score", 0.85)
            .with_duration_ms(150);

        store.log(entry_complete).await.unwrap();

        // Query by correlation ID
        let filter = AuditFilter::new().with_correlation_id(correlation_id);
        let entries = store.query(filter).await.unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.org_id == Some(org_id.to_string())));
    }

    #[tokio::test]
    async fn test_audit_filtering() {
        let store = InMemoryAuditStore::new(1000);

        // Log entries for different orgs
        for i in 0..10 {
            let org = if i % 2 == 0 { "org-a" } else { "org-b" };
            let op = if i % 3 == 0 {
                CTEOperationType::ReceiptMint
            } else {
                CTEOperationType::CoverageComputation
            };

            let entry = AuditEntry::new(op, format!("Entry {}", i))
                .with_org_id(org);

            store.log(entry).await.unwrap();
        }

        // Filter by org
        let filter = AuditFilter::new().with_org_id("org-a");
        let entries = store.query(filter).await.unwrap();
        assert_eq!(entries.len(), 5);

        // Filter by operation
        let filter = AuditFilter::new().with_operation(CTEOperationType::ReceiptMint);
        let entries = store.query(filter).await.unwrap();
        assert_eq!(entries.len(), 4);
    }
}

mod metrics_integration {
    use super::*;
    use metrics::*;
    use std::time::Duration;

    #[test]
    fn test_full_metrics_workflow() {
        let metrics = GlacisMetrics::new();

        // Simulate request processing
        for i in 0..100 {
            let duration = Duration::from_millis(10 + (i % 50));
            metrics.record_request(duration);

            if i % 10 == 0 {
                metrics.record_error();
            }

            if i % 3 == 0 {
                metrics.cache_hits.inc();
            } else {
                metrics.cache_misses.inc();
            }
        }

        // Verify metrics
        assert_eq!(metrics.requests_total.get(), 100);
        assert_eq!(metrics.requests_errors.get(), 10);
        assert!((metrics.error_rate() - 0.1).abs() < 0.01);

        // Check cache hit ratio
        let hit_ratio = metrics.cache_hit_ratio();
        assert!(hit_ratio > 0.3 && hit_ratio < 0.4);

        // Check latency percentiles
        let snapshot = metrics.registry.snapshot();
        let latency = snapshot.histograms.values().next().unwrap();
        assert!(latency.count > 0);
    }

    #[test]
    fn test_concurrent_metrics() {
        use std::sync::Arc;
        use std::thread;

        let metrics = Arc::new(GlacisMetrics::new());

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let m = metrics.clone();
                thread::spawn(move || {
                    for _ in 0..100 {
                        // record_request already increments requests_total
                        m.record_request(Duration::from_millis(5));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(metrics.requests_total.get(), 1000);
    }
}

mod cache_integration {
    use super::*;
    use cache::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_cache_aside_pattern() {
        let cache = Arc::new(LruCache::new(100));
        let aside = CacheAside::new(cache);

        let mut fetch_count = 0;

        // First call should fetch
        let value = aside
            .get_or_fetch(
                &"key1".to_string(),
                || {
                    fetch_count += 1;
                    async { Ok(42i32) }
                },
                Some(Duration::from_secs(60)),
            )
            .await
            .unwrap();

        assert_eq!(value, 42);
        assert_eq!(fetch_count, 1);

        // Second call should use cache
        let value = aside
            .get_or_fetch(
                &"key1".to_string(),
                || {
                    fetch_count += 1;
                    async { Ok(99i32) }
                },
                Some(Duration::from_secs(60)),
            )
            .await
            .unwrap();

        assert_eq!(value, 42); // Cached value
        assert_eq!(fetch_count, 1); // No additional fetch

        // Check stats
        let stats = aside.stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_lru_eviction_under_load() {
        let cache: LruCache<u64, String> = LruCache::new(10);

        // Fill cache
        for i in 0..10 {
            cache.set(i, format!("value-{}", i), None).await;
        }

        // Access first 5 items to make them "recent"
        for i in 0..5 {
            cache.get(&i).await;
        }

        // Add 5 more items, should evict items 5-9
        for i in 10..15 {
            cache.set(i, format!("value-{}", i), None).await;
        }

        // Items 0-4 should still be present (recently accessed)
        for i in 0..5 {
            assert!(cache.get(&i).await.is_some(), "Item {} should exist", i);
        }

        // Items 5-9 should be evicted
        for i in 5..10 {
            assert!(cache.get(&i).await.is_none(), "Item {} should be evicted", i);
        }
    }
}

mod rate_limit_integration {
    use super::*;
    use rate_limit::*;
    use std::time::Duration;

    #[test]
    fn test_rate_limiter_burst_protection() {
        let config = RateLimitConfig {
            max_requests: 100,
            window: Duration::from_secs(60),
            burst_capacity: 10,
            refill_rate: 1.67, // ~100 per minute
        };

        let limiter = RateLimiter::new(config);

        // Should allow burst of 10
        for i in 0..10 {
            let result = limiter.check_org("test-org");
            assert!(result.allowed, "Request {} should be allowed in burst", i);
        }

        // 11th request should be rate limited (burst exceeded)
        let result = limiter.check_org("test-org");
        assert!(!result.allowed, "Request should be rate limited after burst");
        assert!(result.retry_after.is_some());
    }

    #[test]
    fn test_per_endpoint_limits() {
        let limiter = RateLimiter::new(RateLimitConfig::default())
            .with_endpoint_config("/api/expensive", RateLimitConfig::strict());

        // Normal endpoint allows many requests
        for _ in 0..20 {
            let result = limiter.check_endpoint("org-1", "/api/normal");
            assert!(result.allowed);
        }

        // Expensive endpoint is strictly limited
        for i in 0..5 {
            let result = limiter.check_endpoint("org-1", "/api/expensive");
            if i >= 3 {
                assert!(!result.allowed, "Strict endpoint should limit after 3 requests");
            }
        }
    }
}

mod retry_queue_integration {
    use super::*;
    use retry::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_webhook_retry_workflow() {
        let store = InMemoryQueueStore::new();
        let queue = RetryQueue::new(store, RetryQueueConfig::default());

        // Enqueue a webhook
        let payload = WebhookPayload::new(
            "https://example.com/webhook",
            "attestation.created",
            r#"{"id": "123"}"#,
        );

        let id = queue.enqueue(payload).await.unwrap();
        assert!(!id.is_nil());

        // Process successfully
        let processed = queue
            .process(|_| async { ProcessResult::Success })
            .await
            .unwrap();

        assert_eq!(processed, 1);

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.pending, 0);
    }

    #[tokio::test]
    async fn test_retry_with_backoff() {
        let store = InMemoryQueueStore::new();
        let config = RetryQueueConfig {
            max_retries: 3,
            ..Default::default()
        };
        let queue = RetryQueue::new(store, config);

        let attempt_count = Arc::new(AtomicUsize::new(0));

        queue.enqueue("test".to_string()).await.unwrap();

        // Process with failures
        let count = attempt_count.clone();
        queue
            .process(move |_| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    ProcessResult::Retry("temporary error".to_string())
                }
            })
            .await
            .unwrap();

        assert_eq!(attempt_count.load(Ordering::SeqCst), 1);

        // Item should be back in pending with scheduled retry
        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 1);
    }
}

mod key_rotation_integration {
    use super::*;
    use keys::*;

    #[tokio::test]
    async fn test_key_rotation_workflow() {
        let store = InMemoryKeyStore::new();
        let manager = KeyManager::new(store, RotationConfig::default());

        // Create initial key
        let material1 = generate_key_material(32);
        let key1 = manager
            .create_key(KeyType::WebhookSecret, material1, "org-1".to_string())
            .await
            .unwrap();

        assert!(key1.is_active());
        assert_eq!(key1.version, 1);

        // Rotate key
        let material2 = generate_key_material(32);
        let key2 = manager
            .rotate_key("org-1", &KeyType::WebhookSecret, material2)
            .await
            .unwrap();

        assert!(key2.is_active());
        assert_eq!(key2.version, 2);

        // Old key should still be valid for verification
        let verification_keys = manager
            .get_verification_keys("org-1", &KeyType::WebhookSecret)
            .await
            .unwrap();

        assert_eq!(verification_keys.len(), 2);

        // Signing should use latest key
        let signing_key = manager
            .get_signing_key("org-1", &KeyType::WebhookSecret)
            .await
            .unwrap();

        assert_eq!(signing_key.version, 2);
    }

    #[tokio::test]
    async fn test_key_revocation() {
        let store = InMemoryKeyStore::new();
        let manager = KeyManager::new(store, RotationConfig::default());

        let material = generate_key_material(32);
        let key = manager
            .create_key(KeyType::ApiKey, material, "org-1".to_string())
            .await
            .unwrap();

        // Revoke the key
        manager.revoke_key(key.id).await.unwrap();

        // Should not be available for signing
        let result = manager.get_signing_key("org-1", &KeyType::ApiKey).await;
        assert!(result.is_err());

        // Should not be available for verification either
        let keys = manager
            .get_verification_keys("org-1", &KeyType::ApiKey)
            .await
            .unwrap();
        assert!(keys.is_empty());
    }
}

mod import_integration {
    use super::*;
    use import::*;
    use std::io::Cursor;

    #[test]
    fn test_csv_import_with_validation() {
        let csv_data = r#"control_id,title,coverage
AC-1,Access Control Policy,0.95
AC-2,Account Management,0.80
AC-3,Access Enforcement,
"#;

        let importer = CsvImporter::default();
        let mut records = importer.parse(Cursor::new(csv_data)).unwrap();

        // Validate records - check for non-empty coverage values
        let validator = RecordValidator::new()
            .require_field("control_id")
            .add_validator(|r| {
                // Check that coverage is present and valid
                match r.data.get("coverage") {
                    Some(v) if v.as_str() == Some("") => {
                        vec!["Coverage is required".to_string()]
                    }
                    Some(v) => {
                        if let Some(s) = v.as_str() {
                            if let Ok(val) = s.parse::<f64>() {
                                if val < 0.0 || val > 1.0 {
                                    return vec!["Coverage must be between 0 and 1".to_string()];
                                }
                            }
                        } else if let Some(val) = v.as_f64() {
                            if val < 0.0 || val > 1.0 {
                                return vec!["Coverage must be between 0 and 1".to_string()];
                            }
                        }
                        vec![]
                    }
                    None => vec!["Coverage is required".to_string()],
                }
            });

        validator.validate_all(&mut records);

        assert_eq!(records.len(), 3);
        assert!(records[0].valid);
        assert!(records[1].valid);
        // Third record has empty coverage - should be invalid
        assert!(!records[2].valid);
    }

    #[test]
    fn test_oscal_import() {
        let oscal_data = r#"{
            "catalog": {
                "uuid": "test-catalog",
                "groups": [{
                    "id": "ac",
                    "title": "Access Control",
                    "controls": [
                        {
                            "id": "AC-1",
                            "title": "Access Control Policy and Procedures",
                            "parts": [{"name": "statement", "prose": "The organization develops..."}]
                        },
                        {
                            "id": "AC-2",
                            "title": "Account Management",
                            "parts": [{"name": "statement", "prose": "The organization manages..."}]
                        }
                    ]
                }]
            }
        }"#;

        let importer = OscalImporter::default();
        let records = importer.parse_catalog(Cursor::new(oscal_data)).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get_string("control_id"), Some("AC-1".to_string()));
        assert_eq!(records[1].get_string("control_id"), Some("AC-2".to_string()));
        assert!(records[0].get_string("title").is_some());
        assert!(records[0].get_string("description").is_some());
    }

    #[test]
    fn test_import_with_defaults() {
        let json_data = r#"[{"id": 1}, {"id": 2, "status": "active"}]"#;

        let mut defaults = std::collections::HashMap::new();
        defaults.insert("status".to_string(), serde_json::json!("pending"));
        defaults.insert("priority".to_string(), serde_json::json!(5));

        let config = ImportConfig {
            defaults,
            ..Default::default()
        };

        let importer = JsonImporter::new(config);
        let records = importer.parse_json(Cursor::new(json_data)).unwrap();

        // First record should have default status
        assert_eq!(
            records[0].get_string("status"),
            Some("pending".to_string())
        );

        // Second record should keep its own status
        assert_eq!(
            records[1].get_string("status"),
            Some("active".to_string())
        );

        // Both should have default priority
        assert_eq!(records[0].get_i64("priority"), Some(5));
        assert_eq!(records[1].get_i64("priority"), Some(5));
    }
}

mod tracing_integration {
    use super::*;
    use tracing_module::*;

    #[test]
    fn test_request_context_propagation() {
        let header_value = "550e8400-e29b-41d4-a716-446655440000";
        let ctx = RequestContext::from_header(header_value);

        assert_eq!(ctx.correlation_id.to_header_value(), header_value);

        let (header_name, value) = ctx.request_id_header();
        assert_eq!(header_name, "x-request-id");
        assert_eq!(value, header_value);
    }

    #[test]
    fn test_span_hierarchy() {
        let mut ctx = RequestContext::new()
            .with_org("test-org")
            .with_user("test-user");

        let parent_span = ctx.start_span("parent_operation");
        let parent_id = {
            let guard = futures::executor::block_on(parent_span.read());
            guard.span_id
        };

        let child_span = ctx.start_span("child_operation");
        let child_parent_id = {
            let guard = futures::executor::block_on(child_span.read());
            guard.parent_span_id
        };

        assert_eq!(child_parent_id, Some(parent_id));
    }

    #[test]
    fn test_span_timing() {
        let cid = CorrelationId::new();
        let mut span = Span::new("test_operation", cid);

        std::thread::sleep(std::time::Duration::from_millis(10));
        span.finish_ok();

        let duration = span.duration().unwrap();
        assert!(duration >= std::time::Duration::from_millis(10));
        assert_eq!(span.success, Some(true));
    }
}

// Re-export modules for tests
mod tracing_module {
    pub use glacis_core::tracing::*;
}
