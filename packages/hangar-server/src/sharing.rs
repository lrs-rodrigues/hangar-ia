use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The authenticated subject represented by an API key.  API keys remain the
/// alpha identity mechanism, but this distinction prevents an agent grant from
/// also matching a human-user grant that happens to use another key.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    #[default]
    Agent,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ShareAudience {
    /// Every authorized subject in the same organization may retrieve it.
    Organization,
    /// Authorized subjects must make the request in this target workspace.
    Workspace { workspace_id: String },
    /// An API-key identity representing one agent.
    Agent { subject_id: Uuid },
    /// An API-key identity representing one human user.
    User { subject_id: Uuid },
}

impl ShareAudience {
    pub fn validates(&self) -> bool {
        !matches!(self, Self::Workspace { workspace_id } if workspace_id.trim().is_empty())
    }

    pub fn permits(
        &self,
        request_workspace_id: &str,
        subject_id: Uuid,
        subject_kind: SubjectKind,
    ) -> bool {
        match self {
            Self::Organization => true,
            Self::Workspace { workspace_id } => workspace_id == request_workspace_id,
            Self::Agent { subject_id: target } => {
                subject_kind == SubjectKind::Agent && target == &subject_id
            }
            Self::User { subject_id: target } => {
                subject_kind == SubjectKind::User && target == &subject_id
            }
        }
    }

    pub fn stable_key(&self) -> String {
        match self {
            Self::Organization => "organization".into(),
            Self::Workspace { workspace_id } => format!("workspace:{workspace_id}"),
            Self::Agent { subject_id } => format!("agent:{subject_id}"),
            Self::User { subject_id } => format!("user:{subject_id}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShareReviewState {
    Pending,
    Approved,
    Rejected,
    Revoked,
}

impl ShareReviewState {
    pub fn can_transition_to(&self, next: &Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Pending,
                Self::Approved | Self::Rejected | Self::Revoked
            ) | (Self::Approved, Self::Revoked)
        )
    }
}

/// Canonical, tenant-scoped ACL grant for a durable memory.  A grant does not
/// duplicate content: retrieval always reads the source memory and rechecks
/// its lifecycle/expiry before exposing it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryShare {
    pub id: Uuid,
    pub organization_id: String,
    pub source_workspace_id: String,
    pub memory_id: Uuid,
    pub audience: ShareAudience,
    pub state: ShareReviewState,
    pub proposed_by: Uuid,
    pub reviewed_by: Option<Uuid>,
    pub review_note: Option<String>,
    pub source_memory_version: u32,
    pub expires_at_unix_ms: Option<u128>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
    pub version: u32,
}

impl MemoryShare {
    pub fn is_active_for(
        &self,
        request_workspace_id: &str,
        subject_id: Uuid,
        subject_kind: SubjectKind,
        now_unix_ms: u128,
    ) -> bool {
        self.state == ShareReviewState::Approved
            && self
                .expires_at_unix_ms
                .is_none_or(|expires_at| expires_at > now_unix_ms)
            && self
                .audience
                .permits(request_workspace_id, subject_id, subject_kind)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextEvidence {
    pub memory_id: Uuid,
    pub source_workspace_id: String,
    pub source: Option<String>,
    pub content_sha256: String,
    pub memory_version: u32,
    pub share_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextItem {
    pub content: String,
    pub score: usize,
    pub estimated_tokens: usize,
    /// Retrieved material is data, never instructions that change policy or
    /// grant tool authority.
    pub untrusted: bool,
    pub evidence: ContextEvidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPackage {
    pub organization_id: String,
    pub workspace_id: String,
    pub query: String,
    pub token_budget: usize,
    pub estimated_tokens: usize,
    pub items: Vec<ContextItem>,
    pub policy_notice: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audience_never_crosses_subject_or_workspace_boundaries() {
        let identity = Uuid::now_v7();
        assert!(
            ShareAudience::Workspace {
                workspace_id: "payments".into()
            }
            .permits("payments", identity, SubjectKind::Agent)
        );
        assert!(
            !ShareAudience::Workspace {
                workspace_id: "payments".into()
            }
            .permits("security", identity, SubjectKind::Agent)
        );
        assert!(
            ShareAudience::Agent {
                subject_id: identity
            }
            .permits("security", identity, SubjectKind::Agent)
        );
        assert!(
            !ShareAudience::Agent {
                subject_id: identity
            }
            .permits("security", identity, SubjectKind::User)
        );
    }
}
