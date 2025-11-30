//! Structured Audit Logging for Glacis Chain of Custody
//!
//! Provides comprehensive audit logging for CTE operations including:
//! - Coverage computation events
//! - Gap analysis operations
//! - Conflict resolution actions
//! - Receipt minting and verification

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Audit event severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditLevel {
    /// Informational events
    Info,
    /// Warning events that may need attention
    Warning,
    /// Error events requiring investigation
    Error,
    /// Critical security events
    Critical,
}

/// Types of CTE operations that can be audited
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CTEOperationType {
    /// Coverage computation for a policy
    CoverageComputation,
    /// Gap analysis between policies
    GapAnalysis,
    /// Conflict resolution between overlapping coverages
    ConflictResolution,
    /// Receipt minting operation
    ReceiptMint,
    /// Receipt verification
    ReceiptVerify,
    /// Attestation ingestion
    AttestationIngest,
    /// Policy creation
    PolicyCreate,
    /// Policy update
    PolicyUpdate,
    /// Policy deletion
    PolicyDelete,
    /// Key rotation event
    KeyRotation,
    /// Session event
    SessionEvent,
    /// Custom operation type
    Custom(String),
}

/// Outcome of an audited operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Success,
    Failure,
    PartialSuccess,
    Pending,
    Skipped,
}

/// A structured audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique identifier for this audit entry
    pub id: Uuid,
    /// Timestamp when the event occurred
    pub timestamp: DateTime<Utc>,
    /// Correlation ID for request tracing
    pub correlation_id: Option<Uuid>,
    /// Type of operation being audited
    pub operation: CTEOperationType,
    /// Severity level
    pub level: AuditLevel,
    /// Outcome of the operation
    pub outcome: OperationOutcome,
    /// Organization ID if applicable
    pub org_id: Option<String>,
    /// User ID who initiated the operation
    pub user_id: Option<String>,
    /// Resource ID being operated on
    pub resource_id: Option<String>,
    /// Resource type (e.g., "policy", "receipt", "attestation")
    pub resource_type: Option<String>,
    /// Human-readable message
    pub message: String,
    /// Additional structured metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// Duration of the operation in milliseconds
    pub duration_ms: Option<u64>,
    /// IP address of the requester
    pub ip_address: Option<String>,
    /// User agent string
    pub user_agent: Option<String>,
}

impl AuditEntry {
    /// Create a new audit entry with the current timestamp
    pub fn new(operation: CTEOperationType, message: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            correlation_id: None,
            operation,
            level: AuditLevel::Info,
            outcome: OperationOutcome::Success,
            org_id: None,
            user_id: None,
            resource_id: None,
            resource_type: None,
            message: message.into(),
            metadata: HashMap::new(),
            duration_ms: None,
            ip_address: None,
            user_agent: None,
        }
    }

    /// Set the correlation ID for request tracing
    pub fn with_correlation_id(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    /// Set the severity level
    pub fn with_level(mut self, level: AuditLevel) -> Self {
        self.level = level;
        self
    }

    /// Set the operation outcome
    pub fn with_outcome(mut self, outcome: OperationOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// Set the organization ID
    pub fn with_org_id(mut self, org_id: impl Into<String>) -> Self {
        self.org_id = Some(org_id.into());
        self
    }

    /// Set the user ID
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set the resource being operated on
    pub fn with_resource(mut self, resource_type: impl Into<String>, resource_id: impl Into<String>) -> Self {
        self.resource_type = Some(resource_type.into());
        self.resource_id = Some(resource_id.into());
        self
    }

    /// Add metadata to the entry
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.metadata.insert(key.into(), v);
        }
        self
    }

    /// Set the operation duration
    pub fn with_duration_ms(mut self, duration: u64) -> Self {
        self.duration_ms = Some(duration);
        self
    }

    /// Set client information
    pub fn with_client_info(mut self, ip: Option<String>, user_agent: Option<String>) -> Self {
        self.ip_address = ip;
        self.user_agent = user_agent;
        self
    }
}

/// Trait for audit log storage backends
#[async_trait::async_trait]
pub trait AuditStore: Send + Sync {
    /// Log an audit entry
    async fn log(&self, entry: AuditEntry) -> Result<(), AuditError>;

    /// Query audit entries with filters
    async fn query(&self, filter: AuditFilter) -> Result<Vec<AuditEntry>, AuditError>;

    /// Get a specific audit entry by ID
    async fn get(&self, id: Uuid) -> Result<Option<AuditEntry>, AuditError>;
}

/// Filter for querying audit entries
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub correlation_id: Option<Uuid>,
    pub operation: Option<CTEOperationType>,
    pub org_id: Option<String>,
    pub user_id: Option<String>,
    pub resource_id: Option<String>,
    pub level: Option<AuditLevel>,
    pub outcome: Option<OperationOutcome>,
    pub from_timestamp: Option<DateTime<Utc>>,
    pub to_timestamp: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl AuditFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_correlation_id(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    pub fn with_org_id(mut self, org_id: impl Into<String>) -> Self {
        self.org_id = Some(org_id.into());
        self
    }

    pub fn with_operation(mut self, op: CTEOperationType) -> Self {
        self.operation = Some(op);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Errors that can occur during audit operations
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Entry not found: {0}")]
    NotFound(Uuid),
}

/// In-memory audit store for testing and development
pub struct InMemoryAuditStore {
    entries: Arc<RwLock<Vec<AuditEntry>>>,
    max_entries: usize,
}

impl Default for InMemoryAuditStore {
    fn default() -> Self {
        Self::new(10000)
    }
}

impl InMemoryAuditStore {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            max_entries,
        }
    }
}

#[async_trait::async_trait]
impl AuditStore for InMemoryAuditStore {
    async fn log(&self, entry: AuditEntry) -> Result<(), AuditError> {
        let mut entries = self.entries.write().await;
        entries.push(entry);

        // Trim old entries if we exceed max
        if entries.len() > self.max_entries {
            let excess = entries.len() - self.max_entries;
            entries.drain(0..excess);
        }

        Ok(())
    }

    async fn query(&self, filter: AuditFilter) -> Result<Vec<AuditEntry>, AuditError> {
        let entries = self.entries.read().await;
        let mut results: Vec<_> = entries
            .iter()
            .filter(|e| {
                if let Some(ref cid) = filter.correlation_id {
                    if e.correlation_id.as_ref() != Some(cid) {
                        return false;
                    }
                }
                if let Some(ref org) = filter.org_id {
                    if e.org_id.as_ref() != Some(org) {
                        return false;
                    }
                }
                if let Some(ref op) = filter.operation {
                    if &e.operation != op {
                        return false;
                    }
                }
                if let Some(ref level) = filter.level {
                    if &e.level != level {
                        return false;
                    }
                }
                if let Some(ref from) = filter.from_timestamp {
                    if &e.timestamp < from {
                        return false;
                    }
                }
                if let Some(ref to) = filter.to_timestamp {
                    if &e.timestamp > to {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Apply offset and limit
        if let Some(offset) = filter.offset {
            results = results.into_iter().skip(offset).collect();
        }
        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn get(&self, id: Uuid) -> Result<Option<AuditEntry>, AuditError> {
        let entries = self.entries.read().await;
        Ok(entries.iter().find(|e| e.id == id).cloned())
    }
}

/// Helper function to log a CTE operation
pub async fn log_cte_operation<S: AuditStore>(
    store: &S,
    operation: CTEOperationType,
    correlation_id: Option<Uuid>,
    org_id: Option<String>,
    resource_type: Option<String>,
    resource_id: Option<String>,
    outcome: OperationOutcome,
    message: impl Into<String>,
    metadata: Option<HashMap<String, serde_json::Value>>,
) -> Result<Uuid, AuditError> {
    let mut entry = AuditEntry::new(operation, message)
        .with_outcome(outcome);

    if let Some(cid) = correlation_id {
        entry = entry.with_correlation_id(cid);
    }
    if let Some(org) = org_id {
        entry = entry.with_org_id(org);
    }
    if let (Some(rt), Some(rid)) = (resource_type, resource_id) {
        entry = entry.with_resource(rt, rid);
    }
    if let Some(meta) = metadata {
        entry.metadata = meta;
    }

    let id = entry.id;
    store.log(entry).await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audit_entry_creation() {
        let entry = AuditEntry::new(CTEOperationType::CoverageComputation, "Computing coverage")
            .with_org_id("org-123")
            .with_user_id("user-456")
            .with_resource("policy", "pol-789")
            .with_metadata("framework", "SOC2");

        assert_eq!(entry.org_id, Some("org-123".to_string()));
        assert_eq!(entry.resource_type, Some("policy".to_string()));
        assert!(entry.metadata.contains_key("framework"));
    }

    #[tokio::test]
    async fn test_in_memory_store() {
        let store = InMemoryAuditStore::new(100);

        let entry = AuditEntry::new(CTEOperationType::ReceiptMint, "Minted receipt")
            .with_org_id("test-org");

        store.log(entry.clone()).await.unwrap();

        let filter = AuditFilter::new().with_org_id("test-org");
        let results = store.query(filter).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].org_id, Some("test-org".to_string()));
    }

    #[tokio::test]
    async fn test_store_max_entries() {
        let store = InMemoryAuditStore::new(5);

        for i in 0..10 {
            let entry = AuditEntry::new(CTEOperationType::ReceiptMint, format!("Entry {}", i));
            store.log(entry).await.unwrap();
        }

        let results = store.query(AuditFilter::default()).await.unwrap();
        assert_eq!(results.len(), 5);
    }
}
