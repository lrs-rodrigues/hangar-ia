use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MEMORIES: TableDefinition<&str, &[u8]> = TableDefinition::new("memories");
const BLOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("blobs");
const API_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("api_keys");
const AUDIT_EVENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("audit_events");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub organization_id: String,
    pub workspace_id: String,
    pub content: String,
    pub source: Option<String>,
    pub content_sha256: String,
    pub created_by: Uuid,
    pub confidence: f32,
    pub lifecycle: MemoryLifecycle,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
    pub expires_at_unix_ms: Option<u128>,
    pub superseded_by: Option<Uuid>,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLifecycle {
    Proposed,
    Validated,
    Published,
    Superseded,
    Expired,
}

pub struct NewMemory {
    pub organization_id: String,
    pub workspace_id: String,
    pub content: String,
    pub source: Option<String>,
    pub created_by: Uuid,
    pub confidence: f32,
}

pub struct MemoryTransition {
    pub lifecycle: MemoryLifecycle,
    pub expires_at_unix_ms: Option<u128>,
    pub superseded_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobReceipt {
    pub sha256: String,
    pub bytes: usize,
    pub media_type: String,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Writer,
    Reader,
}

impl Role {
    pub fn allows(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::Owner, _)
                | (Self::Writer, Self::Writer | Self::Reader)
                | (Self::Reader, Self::Reader)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: Uuid,
    pub organization_id: String,
    pub workspace_id: Option<String>,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssuedApiKey {
    pub id: Uuid,
    pub token: String,
    pub organization_id: String,
    pub workspace_id: Option<String>,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiKeyRecord {
    principal: Principal,
    token_hash: String,
    created_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditEvent {
    id: Uuid,
    principal_id: Uuid,
    organization_id: String,
    workspace_id: Option<String>,
    action: String,
    resource: String,
    created_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlobManifest {
    organization_id: String,
    workspace_id: String,
    sha256: String,
    bytes: usize,
    media_type: String,
}

pub struct HangarStore {
    database: Database,
    blobs_dir: PathBuf,
}

impl HangarStore {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(data_dir)?;
        let blobs_dir = data_dir.join("blobs");
        fs::create_dir_all(&blobs_dir)?;
        let database = Database::create(data_dir.join("canonical.redb"))?;
        Ok(Self {
            database,
            blobs_dir,
        })
    }

    pub fn create_memory(&mut self, new: NewMemory) -> anyhow::Result<Memory> {
        anyhow::ensure!(
            !new.organization_id.trim().is_empty(),
            "organization_id cannot be empty"
        );
        anyhow::ensure!(
            !new.workspace_id.trim().is_empty(),
            "workspace_id cannot be empty"
        );
        anyhow::ensure!(!new.content.trim().is_empty(), "content cannot be empty");
        anyhow::ensure!(
            (0.0..=1.0).contains(&new.confidence),
            "confidence must be between 0 and 1"
        );
        let now = now_unix_ms()?;
        let content_sha256 = hex::encode(Sha256::digest(new.content.as_bytes()));
        let memory = Memory {
            id: Uuid::now_v7(),
            organization_id: new.organization_id,
            workspace_id: new.workspace_id,
            content: new.content,
            source: new.source,
            content_sha256,
            created_by: new.created_by,
            confidence: new.confidence,
            lifecycle: MemoryLifecycle::Proposed,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
            expires_at_unix_ms: None,
            superseded_by: None,
            version: 1,
        };
        let encoded = serde_json::to_vec(&memory)?;
        let transaction = self.database.begin_write()?;
        {
            transaction
                .open_table(MEMORIES)?
                .insert(memory.id.to_string().as_str(), encoded.as_slice())?;
        }
        transaction.commit()?;
        Ok(memory)
    }

    pub fn transition_memory(
        &mut self,
        id: Uuid,
        organization_id: &str,
        workspace_id: &str,
        transition: MemoryTransition,
    ) -> anyhow::Result<Option<Memory>> {
        let key = id.to_string();
        let transaction = self.database.begin_write()?;
        let mut table = transaction.open_table(MEMORIES)?;
        let mut memory: Memory = {
            let Some(value) = table.get(key.as_str())? else {
                return Ok(None);
            };
            serde_json::from_slice(value.value())?
        };
        anyhow::ensure!(
            memory.organization_id == organization_id && memory.workspace_id == workspace_id,
            "memory not found in this workspace"
        );
        anyhow::ensure!(
            is_valid_transition(&memory.lifecycle, &transition.lifecycle),
            "invalid lifecycle transition"
        );
        if matches!(transition.lifecycle, MemoryLifecycle::Superseded) {
            let replacement = transition
                .superseded_by
                .context("superseded memory requires superseded_by")?;
            anyhow::ensure!(replacement != memory.id, "a memory cannot supersede itself");
            let replacement_memory: Memory = {
                let replacement_key = replacement.to_string();
                let replacement_value = table
                    .get(replacement_key.as_str())?
                    .context("replacement memory not found")?;
                serde_json::from_slice(replacement_value.value())?
            };
            anyhow::ensure!(
                replacement_memory.organization_id == organization_id
                    && replacement_memory.workspace_id == workspace_id,
                "replacement memory must be in the same workspace"
            );
        }
        if let Some(expires_at) = transition.expires_at_unix_ms {
            anyhow::ensure!(
                expires_at > now_unix_ms()?,
                "expires_at_unix_ms must be in the future"
            );
        }
        memory.lifecycle = transition.lifecycle;
        memory.expires_at_unix_ms = transition.expires_at_unix_ms.or(memory.expires_at_unix_ms);
        memory.superseded_by = transition.superseded_by;
        memory.updated_at_unix_ms = now_unix_ms()?;
        memory.version += 1;
        let encoded = serde_json::to_vec(&memory)?;
        table.insert(key.as_str(), encoded.as_slice())?;
        drop(table);
        transaction.commit()?;
        Ok(Some(memory))
    }

    pub fn issue_api_key(
        &mut self,
        organization_id: String,
        workspace_id: Option<String>,
        role: Role,
    ) -> anyhow::Result<IssuedApiKey> {
        anyhow::ensure!(
            !organization_id.trim().is_empty(),
            "organization_id cannot be empty"
        );
        if let Some(workspace_id) = &workspace_id {
            anyhow::ensure!(
                !workspace_id.trim().is_empty(),
                "workspace_id cannot be empty"
            );
        }
        let id = Uuid::now_v7();
        let token = format!("hk_{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple());
        let token_hash = hash_token(&token);
        let principal = Principal {
            id,
            organization_id: organization_id.clone(),
            workspace_id: workspace_id.clone(),
            role,
        };
        let record = ApiKeyRecord {
            principal,
            token_hash: token_hash.clone(),
            created_at_unix_ms: now_unix_ms()?,
        };
        let encoded = serde_json::to_vec(&record)?;
        let transaction = self.database.begin_write()?;
        transaction
            .open_table(API_KEYS)?
            .insert(token_hash.as_str(), encoded.as_slice())?;
        transaction.commit()?;
        Ok(IssuedApiKey {
            id,
            token,
            organization_id,
            workspace_id,
            role,
        })
    }

    pub fn authenticate(&self, token: &str) -> anyhow::Result<Option<Principal>> {
        let transaction = self.database.begin_read()?;
        let table = transaction.open_table(API_KEYS)?;
        let Some(value) = table.get(hash_token(token).as_str())? else {
            return Ok(None);
        };
        let record: ApiKeyRecord = serde_json::from_slice(value.value())?;
        Ok(Some(record.principal))
    }

    pub fn audit(
        &mut self,
        principal: &Principal,
        action: &str,
        resource: &str,
    ) -> anyhow::Result<()> {
        let event = AuditEvent {
            id: Uuid::now_v7(),
            principal_id: principal.id,
            organization_id: principal.organization_id.clone(),
            workspace_id: principal.workspace_id.clone(),
            action: action.to_owned(),
            resource: resource.to_owned(),
            created_at_unix_ms: now_unix_ms()?,
        };
        let encoded = serde_json::to_vec(&event)?;
        let transaction = self.database.begin_write()?;
        transaction
            .open_table(AUDIT_EVENTS)?
            .insert(event.id.to_string().as_str(), encoded.as_slice())?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_memory(
        &self,
        id: Uuid,
        organization_id: &str,
        workspace_id: &str,
    ) -> anyhow::Result<Option<Memory>> {
        let transaction = self.database.begin_read()?;
        let table = transaction.open_table(MEMORIES)?;
        let Some(value) = table.get(id.to_string().as_str())? else {
            return Ok(None);
        };
        let memory: Memory = serde_json::from_slice(value.value())?;
        Ok(
            (memory.organization_id == organization_id && memory.workspace_id == workspace_id)
                .then_some(memory),
        )
    }

    pub fn retrieve(
        &self,
        organization_id: &str,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<Memory>> {
        let terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        anyhow::ensure!(!terms.is_empty(), "query cannot be empty");
        let transaction = self.database.begin_read()?;
        let table = transaction.open_table(MEMORIES)?;
        let mut scored = Vec::new();
        for row in table.iter()? {
            let (_, value) = row?;
            let memory: Memory = serde_json::from_slice(value.value())?;
            if memory.organization_id != organization_id || memory.workspace_id != workspace_id {
                continue;
            }
            if !is_retrievable(&memory) {
                continue;
            }
            let haystack = memory.content.to_lowercase();
            let score = terms
                .iter()
                .map(|term| haystack.matches(term).count())
                .sum::<usize>();
            if score > 0 {
                scored.push((score, memory));
            }
        }
        scored.sort_by(|(left, _), (right, _)| right.cmp(left));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, memory)| memory)
            .collect())
    }

    pub fn put_blob(
        &mut self,
        organization_id: &str,
        workspace_id: &str,
        media_type: &str,
        bytes: &[u8],
        sha256: String,
    ) -> anyhow::Result<BlobReceipt> {
        anyhow::ensure!(
            !organization_id.trim().is_empty(),
            "organization_id cannot be empty"
        );
        anyhow::ensure!(
            !workspace_id.trim().is_empty(),
            "workspace_id cannot be empty"
        );
        let expected = hex::encode(Sha256::digest(bytes));
        anyhow::ensure!(expected == sha256, "blob digest mismatch");
        let path = self.blobs_dir.join(&sha256);
        let deduplicated = path.exists();
        if !deduplicated {
            fs::write(&path, bytes).with_context(|| format!("writing blob {}", path.display()))?;
        }
        let manifest = BlobManifest {
            organization_id: organization_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            sha256: sha256.clone(),
            bytes: bytes.len(),
            media_type: media_type.to_owned(),
        };
        let encoded = serde_json::to_vec(&manifest)?;
        let key = format!("{organization_id}:{workspace_id}:{sha256}");
        let transaction = self.database.begin_write()?;
        {
            transaction
                .open_table(BLOBS)?
                .insert(key.as_str(), encoded.as_slice())?;
        }
        transaction.commit()?;
        Ok(BlobReceipt {
            sha256,
            bytes: bytes.len(),
            media_type: media_type.to_owned(),
            deduplicated,
        })
    }
}

fn is_valid_transition(current: &MemoryLifecycle, target: &MemoryLifecycle) -> bool {
    matches!(
        (current, target),
        (MemoryLifecycle::Proposed, MemoryLifecycle::Validated)
            | (MemoryLifecycle::Validated, MemoryLifecycle::Published)
            | (
                MemoryLifecycle::Proposed | MemoryLifecycle::Validated | MemoryLifecycle::Published,
                MemoryLifecycle::Expired
            )
            | (MemoryLifecycle::Published, MemoryLifecycle::Superseded)
    )
}

fn is_retrievable(memory: &Memory) -> bool {
    matches!(memory.lifecycle, MemoryLifecycle::Published)
        && memory
            .expires_at_unix_ms
            .is_none_or(|expires_at| expires_at > now_unix_ms().unwrap_or_default())
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn now_unix_ms() -> anyhow::Result<u128> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolates_memory_by_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let mut store = HangarStore::open(temporary.path()).unwrap();
        let memory = store
            .create_memory(NewMemory {
                organization_id: "acme".into(),
                workspace_id: "payments".into(),
                content: "Use OIDC for service authentication".into(),
                source: None,
                created_by: Uuid::now_v7(),
                confidence: 0.9,
            })
            .unwrap();
        assert!(
            store
                .get_memory(memory.id, "acme", "payments")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .get_memory(memory.id, "acme", "other")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .retrieve("acme", "other", "OIDC", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn content_addressed_blobs_are_deduplicated() {
        let temporary = tempfile::tempdir().unwrap();
        let mut store = HangarStore::open(temporary.path()).unwrap();
        let bytes = b"design decision";
        let digest = hex::encode(Sha256::digest(bytes));
        assert!(
            !store
                .put_blob("acme", "core", "text/plain", bytes, digest.clone())
                .unwrap()
                .deduplicated
        );
        assert!(
            store
                .put_blob("acme", "core", "text/plain", bytes, digest)
                .unwrap()
                .deduplicated
        );
    }

    #[test]
    fn api_key_is_stored_only_as_a_hash() {
        let temporary = tempfile::tempdir().unwrap();
        let mut store = HangarStore::open(temporary.path()).unwrap();
        let key = store
            .issue_api_key("acme".into(), Some("payments".into()), Role::Writer)
            .unwrap();
        let principal = store.authenticate(&key.token).unwrap().unwrap();
        assert_eq!(principal.organization_id, "acme");
        assert!(principal.role.allows(Role::Reader));
        assert!(!principal.role.allows(Role::Owner));
    }

    #[test]
    fn only_published_memory_is_retrievable() {
        let temporary = tempfile::tempdir().unwrap();
        let mut store = HangarStore::open(temporary.path()).unwrap();
        let memory = store
            .create_memory(NewMemory {
                organization_id: "acme".into(),
                workspace_id: "payments".into(),
                content: "Rotate signing keys every ninety days".into(),
                source: Some("security-policy".into()),
                created_by: Uuid::now_v7(),
                confidence: 1.0,
            })
            .unwrap();
        assert!(
            store
                .retrieve("acme", "payments", "signing", 10)
                .unwrap()
                .is_empty()
        );
        store
            .transition_memory(
                memory.id,
                "acme",
                "payments",
                MemoryTransition {
                    lifecycle: MemoryLifecycle::Validated,
                    expires_at_unix_ms: None,
                    superseded_by: None,
                },
            )
            .unwrap();
        store
            .transition_memory(
                memory.id,
                "acme",
                "payments",
                MemoryTransition {
                    lifecycle: MemoryLifecycle::Published,
                    expires_at_unix_ms: None,
                    superseded_by: None,
                },
            )
            .unwrap();
        assert_eq!(
            store
                .retrieve("acme", "payments", "signing", 10)
                .unwrap()
                .len(),
            1
        );
    }
}
