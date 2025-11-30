//! Key Rotation Mechanism
//!
//! Provides secure key management with:
//! - Versioned keys with grace periods
//! - Zero-downtime key rotation
//! - Support for webhook secrets and encryption keys
//! - Automatic key expiration handling

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

/// The current version of a key (monotonically increasing)
pub type KeyVersion = u64;

/// Types of keys that can be managed
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyType {
    /// Webhook signing/verification secret
    WebhookSecret,
    /// Data encryption key
    EncryptionKey,
    /// API authentication key
    ApiKey,
    /// Session signing key
    SessionKey,
    /// Custom key type
    Custom(String),
}

/// Status of a key version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    /// Key is active and should be used for new operations
    Active,
    /// Key is in grace period (valid for verification, not for signing)
    GracePeriod,
    /// Key is deprecated but still valid for verification
    Deprecated,
    /// Key has been revoked and should not be accepted
    Revoked,
    /// Key has expired
    Expired,
}

/// A versioned key with lifecycle information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedKey {
    /// Unique identifier for this key instance
    pub id: Uuid,
    /// The key type
    pub key_type: KeyType,
    /// Version number (higher = newer)
    pub version: KeyVersion,
    /// The actual key material (encrypted at rest)
    #[serde(skip_serializing)]
    pub material: Vec<u8>,
    /// Current status
    pub status: KeyStatus,
    /// When the key was created
    pub created_at: DateTime<Utc>,
    /// When the key becomes active
    pub active_from: DateTime<Utc>,
    /// When the key enters grace period (optional)
    pub grace_period_start: Option<DateTime<Utc>>,
    /// When the key expires completely
    pub expires_at: Option<DateTime<Utc>>,
    /// Organization ID this key belongs to
    pub org_id: String,
    /// Optional metadata
    pub metadata: HashMap<String, String>,
}

impl VersionedKey {
    /// Create a new active key
    pub fn new(key_type: KeyType, material: Vec<u8>, org_id: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            key_type,
            version: 1,
            material,
            status: KeyStatus::Active,
            created_at: now,
            active_from: now,
            grace_period_start: None,
            expires_at: None,
            org_id,
            metadata: HashMap::new(),
        }
    }

    /// Set the expiration time
    pub fn with_expiration(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set a grace period before expiration
    pub fn with_grace_period(mut self, grace_start: DateTime<Utc>) -> Self {
        self.grace_period_start = Some(grace_start);
        self
    }

    /// Check if the key is currently valid for signing/encrypting
    pub fn is_active(&self) -> bool {
        self.status == KeyStatus::Active && self.is_time_valid()
    }

    /// Check if the key is valid for verification/decryption
    pub fn is_valid_for_verification(&self) -> bool {
        matches!(
            self.status,
            KeyStatus::Active | KeyStatus::GracePeriod | KeyStatus::Deprecated
        ) && self.is_time_valid()
    }

    /// Check if the key's time constraints are satisfied
    fn is_time_valid(&self) -> bool {
        let now = Utc::now();
        if now < self.active_from {
            return false;
        }
        if let Some(expires) = self.expires_at {
            if now > expires {
                return false;
            }
        }
        true
    }

    /// Update status based on current time
    pub fn update_status(&mut self) {
        let now = Utc::now();

        // Check expiration
        if let Some(expires) = self.expires_at {
            if now > expires {
                self.status = KeyStatus::Expired;
                return;
            }
        }

        // Check grace period
        if let Some(grace_start) = self.grace_period_start {
            if now >= grace_start && self.status == KeyStatus::Active {
                self.status = KeyStatus::GracePeriod;
            }
        }
    }
}

/// Configuration for key rotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationConfig {
    /// How long before expiration to start grace period
    pub grace_period: Duration,
    /// How long a key remains valid after rotation
    pub overlap_period: Duration,
    /// Whether to automatically rotate on schedule
    pub auto_rotate: bool,
    /// Rotation interval for auto-rotation
    pub rotation_interval: Option<Duration>,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(24 * 60 * 60), // 24 hours
            overlap_period: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
            auto_rotate: false,
            rotation_interval: None,
        }
    }
}

/// Errors that can occur during key operations
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("Key not found: {0}")]
    NotFound(String),

    #[error("Key expired: {0}")]
    Expired(Uuid),

    #[error("Key revoked: {0}")]
    Revoked(Uuid),

    #[error("No active key available for {0:?}")]
    NoActiveKey(KeyType),

    #[error("Invalid key material")]
    InvalidMaterial,

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Rotation in progress")]
    RotationInProgress,
}

/// Trait for key storage backends
#[async_trait::async_trait]
pub trait KeyStore: Send + Sync {
    /// Store a new key
    async fn store(&self, key: VersionedKey) -> Result<(), KeyError>;

    /// Get a key by ID
    async fn get(&self, id: Uuid) -> Result<Option<VersionedKey>, KeyError>;

    /// Get the current active key for an org and type
    async fn get_active(&self, org_id: &str, key_type: &KeyType) -> Result<Option<VersionedKey>, KeyError>;

    /// Get all keys for an org and type (for verification)
    async fn get_all(&self, org_id: &str, key_type: &KeyType) -> Result<Vec<VersionedKey>, KeyError>;

    /// Update a key's status
    async fn update_status(&self, id: Uuid, status: KeyStatus) -> Result<(), KeyError>;

    /// Delete a key
    async fn delete(&self, id: Uuid) -> Result<(), KeyError>;
}

/// In-memory key store for testing
pub struct InMemoryKeyStore {
    keys: Arc<RwLock<HashMap<Uuid, VersionedKey>>>,
}

impl Default for InMemoryKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryKeyStore {
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl KeyStore for InMemoryKeyStore {
    async fn store(&self, key: VersionedKey) -> Result<(), KeyError> {
        let mut keys = self.keys.write().await;
        keys.insert(key.id, key);
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<VersionedKey>, KeyError> {
        let keys = self.keys.read().await;
        Ok(keys.get(&id).cloned())
    }

    async fn get_active(&self, org_id: &str, key_type: &KeyType) -> Result<Option<VersionedKey>, KeyError> {
        let keys = self.keys.read().await;
        let active = keys
            .values()
            .filter(|k| &k.org_id == org_id && &k.key_type == key_type && k.is_active())
            .max_by_key(|k| k.version)
            .cloned();
        Ok(active)
    }

    async fn get_all(&self, org_id: &str, key_type: &KeyType) -> Result<Vec<VersionedKey>, KeyError> {
        let keys = self.keys.read().await;
        let matching: Vec<_> = keys
            .values()
            .filter(|k| &k.org_id == org_id && &k.key_type == key_type)
            .cloned()
            .collect();
        Ok(matching)
    }

    async fn update_status(&self, id: Uuid, status: KeyStatus) -> Result<(), KeyError> {
        let mut keys = self.keys.write().await;
        if let Some(key) = keys.get_mut(&id) {
            key.status = status;
            Ok(())
        } else {
            Err(KeyError::NotFound(id.to_string()))
        }
    }

    async fn delete(&self, id: Uuid) -> Result<(), KeyError> {
        let mut keys = self.keys.write().await;
        keys.remove(&id);
        Ok(())
    }
}

/// Key rotation manager
pub struct KeyManager<S: KeyStore> {
    store: S,
    config: RotationConfig,
}

impl<S: KeyStore> KeyManager<S> {
    pub fn new(store: S, config: RotationConfig) -> Self {
        Self { store, config }
    }

    /// Generate and store a new key
    pub async fn create_key(
        &self,
        key_type: KeyType,
        material: Vec<u8>,
        org_id: String,
    ) -> Result<VersionedKey, KeyError> {
        let key = VersionedKey::new(key_type, material, org_id);
        self.store.store(key.clone()).await?;
        Ok(key)
    }

    /// Rotate a key, creating a new version
    pub async fn rotate_key(
        &self,
        org_id: &str,
        key_type: &KeyType,
        new_material: Vec<u8>,
    ) -> Result<VersionedKey, KeyError> {
        // Get current active key
        let current = self.store.get_active(org_id, key_type).await?;

        let new_version = current.as_ref().map(|k| k.version + 1).unwrap_or(1);

        // Mark old key as grace period
        if let Some(mut old_key) = current {
            old_key.status = KeyStatus::GracePeriod;
            let grace_end = Utc::now() + chrono::Duration::from_std(self.config.overlap_period)
                .unwrap_or(chrono::Duration::days(7));
            old_key.expires_at = Some(grace_end);
            self.store.store(old_key).await?;
        }

        // Create new key
        let mut new_key = VersionedKey::new(key_type.clone(), new_material, org_id.to_string());
        new_key.version = new_version;
        self.store.store(new_key.clone()).await?;

        Ok(new_key)
    }

    /// Get the active key for signing/encrypting
    pub async fn get_signing_key(
        &self,
        org_id: &str,
        key_type: &KeyType,
    ) -> Result<VersionedKey, KeyError> {
        self.store
            .get_active(org_id, key_type)
            .await?
            .ok_or_else(|| KeyError::NoActiveKey(key_type.clone()))
    }

    /// Get all valid keys for verification/decryption
    pub async fn get_verification_keys(
        &self,
        org_id: &str,
        key_type: &KeyType,
    ) -> Result<Vec<VersionedKey>, KeyError> {
        let all_keys = self.store.get_all(org_id, key_type).await?;
        Ok(all_keys.into_iter().filter(|k| k.is_valid_for_verification()).collect())
    }

    /// Revoke a specific key version
    pub async fn revoke_key(&self, key_id: Uuid) -> Result<(), KeyError> {
        self.store.update_status(key_id, KeyStatus::Revoked).await
    }

    /// Clean up expired keys
    pub async fn cleanup_expired(&self, org_id: &str, key_type: &KeyType) -> Result<usize, KeyError> {
        let all_keys = self.store.get_all(org_id, key_type).await?;
        let mut cleaned = 0;

        for mut key in all_keys {
            key.update_status();
            if key.status == KeyStatus::Expired {
                self.store.delete(key.id).await?;
                cleaned += 1;
            }
        }

        Ok(cleaned)
    }
}

/// Generate random key material
pub fn generate_key_material(length: usize) -> Vec<u8> {
    use rand::RngCore;
    let mut material = vec![0u8; length];
    rand::thread_rng().fill_bytes(&mut material);
    material
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_key_creation() {
        let store = InMemoryKeyStore::new();
        let manager = KeyManager::new(store, RotationConfig::default());

        let material = generate_key_material(32);
        let key = manager
            .create_key(KeyType::WebhookSecret, material, "org-123".to_string())
            .await
            .unwrap();

        assert_eq!(key.version, 1);
        assert!(key.is_active());
        assert_eq!(key.org_id, "org-123");
    }

    #[tokio::test]
    async fn test_key_rotation() {
        let store = InMemoryKeyStore::new();
        let manager = KeyManager::new(store, RotationConfig::default());

        // Create initial key
        let material1 = generate_key_material(32);
        let key1 = manager
            .create_key(KeyType::WebhookSecret, material1, "org-123".to_string())
            .await
            .unwrap();

        // Rotate key
        let material2 = generate_key_material(32);
        let key2 = manager
            .rotate_key("org-123", &KeyType::WebhookSecret, material2)
            .await
            .unwrap();

        assert_eq!(key2.version, 2);
        assert!(key2.is_active());

        // Both keys should be valid for verification
        let verification_keys = manager
            .get_verification_keys("org-123", &KeyType::WebhookSecret)
            .await
            .unwrap();

        assert_eq!(verification_keys.len(), 2);
    }

    #[tokio::test]
    async fn test_key_revocation() {
        let store = InMemoryKeyStore::new();
        let manager = KeyManager::new(store, RotationConfig::default());

        let material = generate_key_material(32);
        let key = manager
            .create_key(KeyType::ApiKey, material, "org-123".to_string())
            .await
            .unwrap();

        manager.revoke_key(key.id).await.unwrap();

        // Revoked key should not be available for signing
        let result = manager.get_signing_key("org-123", &KeyType::ApiKey).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_key_status_lifecycle() {
        let now = Utc::now();
        let grace_start = now + chrono::Duration::hours(1);
        let expires = now + chrono::Duration::hours(2);

        let mut key = VersionedKey::new(KeyType::SessionKey, vec![1, 2, 3], "org".to_string())
            .with_grace_period(grace_start)
            .with_expiration(expires);

        assert!(key.is_active());
        assert!(key.is_valid_for_verification());
    }

    #[test]
    fn test_generate_key_material() {
        let material = generate_key_material(32);
        assert_eq!(material.len(), 32);

        // Ensure randomness (extremely unlikely to be all zeros)
        assert!(material.iter().any(|&b| b != 0));
    }
}
