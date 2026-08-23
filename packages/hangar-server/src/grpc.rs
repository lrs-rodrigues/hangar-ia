//! Native gRPC edge for the Hangar core.
//!
//! This module intentionally delegates all identity, authorization, lifecycle,
//! retrieval, and audit behavior to the same `HangarStore` used by HTTP.

use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    AppState,
    sharing::ContextPackage,
    store::{
        GuardrailAction, Memory, MemoryLifecycle, MemoryProvenance, MemoryRetention, NewMemory,
        NewWorkingMemoryEntry, NewWorkingSession, Principal, RetrievedChunk, Role,
        WorkingMemoryKind, WorkingSession,
    },
};

pub mod api {
    tonic::include_proto!("hangar.v1");
}

use api::{
    AppendWorkingMemoryRequest, ContextPackageRequest, CreateMemoryRequest,
    CreateWorkingSessionRequest, DocumentRetrievalResponse, GetMemoryRequest,
    GetWorkingSessionRequest, HealthRequest, HealthResponse, MemoryRetrievalResponse,
    PromoteWorkingMemoryRequest, RetrieveRequest, Scope, UpdateWorkingSummaryRequest,
    hangar_service_server::HangarService,
};

#[derive(Clone)]
pub struct GrpcApi {
    state: AppState,
}

impl GrpcApi {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn authorize<T>(
        &self,
        request: &Request<T>,
        scope: &Scope,
        role: Role,
    ) -> Result<Principal, Status> {
        if scope.organization_id.trim().is_empty() || scope.workspace_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "organization_id and workspace_id are required",
            ));
        }
        let token = request
            .metadata()
            .get("authorization")
            .ok_or_else(|| Status::unauthenticated("missing authorization metadata"))?
            .to_str()
            .map_err(|_| Status::unauthenticated("invalid authorization metadata"))?
            .strip_prefix("Bearer ")
            .filter(|token| !token.is_empty())
            .ok_or_else(|| Status::unauthenticated("expected Bearer API key"))?;
        let store = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?;
        let principal = store
            .authenticate(token)
            .map_err(internal_status)?
            .ok_or_else(|| Status::unauthenticated("invalid API key"))?;
        if principal.organization_id != scope.organization_id || !principal.role.allows(role) {
            return Err(Status::permission_denied(
                "API key is not permitted for this operation",
            ));
        }
        if let Some(workspace_id) = &principal.workspace_id
            && workspace_id != &scope.workspace_id
        {
            return Err(Status::permission_denied(
                "API key is not permitted for this workspace",
            ));
        }
        Ok(principal)
    }

    fn audit(
        &self,
        principal: &Principal,
        scope: &Scope,
        action: &str,
        target: &str,
    ) -> Result<(), Status> {
        let mut store = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?;
        if principal.organization_id != scope.organization_id {
            return Err(Status::internal("principal scope changed during request"));
        }
        store
            .audit(principal, action, target)
            .map_err(internal_status)
    }

    fn enforce_guardrail(
        &self,
        principal: &Principal,
        scope: &Scope,
        action: GuardrailAction,
        target: &str,
    ) -> Result<(), Status> {
        let mut store = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?;
        let decision = store
            .evaluate_guardrail(
                &principal.organization_id,
                &scope.workspace_id,
                principal.role,
                action,
                target,
            )
            .map_err(internal_status)?;
        let audit_action = if decision.allowed {
            "guardrail.decision.allowed"
        } else {
            "guardrail.decision.denied"
        };
        store
            .audit(
                principal,
                audit_action,
                &format!("{}:{}", decision.action.as_str(), decision.target),
            )
            .map_err(internal_status)?;
        if decision.allowed {
            Ok(())
        } else {
            Err(Status::permission_denied(format!(
                "guardrail denied request: {}",
                decision.reason
            )))
        }
    }
}

#[tonic::async_trait]
impl HangarService for GrpcApi {
    async fn get_health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: "ok".into(),
            storage: "embedded".into(),
        }))
    }

    async fn create_memory(
        &self,
        request: Request<CreateMemoryRequest>,
    ) -> Result<Response<api::Memory>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Writer)?;
        let input = request.into_inner();
        let mut store = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?;
        let memory = store
            .create_memory_with_limits(
                NewMemory {
                    organization_id: scope.organization_id.clone(),
                    workspace_id: scope.workspace_id.clone(),
                    content: input.content,
                    source: input.source,
                    created_by: principal.id,
                    confidence: input.confidence.unwrap_or(1.0),
                    expires_at_unix_ms: input.expires_at_unix_ms.map(u128::from),
                    provenance: crate::store::MemoryProvenance::Direct,
                },
                self.state.limits,
            )
            .map_err(internal_status)?;
        store
            .audit(&principal, "memory.create", &memory.id.to_string())
            .map_err(internal_status)?;
        Ok(Response::new(memory_proto(memory)))
    }

    async fn get_memory(
        &self,
        request: Request<GetMemoryRequest>,
    ) -> Result<Response<api::Memory>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Reader)?;
        self.enforce_guardrail(&principal, &scope, GuardrailAction::MemoryRead, "memory")?;
        let input = request.into_inner();
        let id = Uuid::parse_str(&input.memory_id)
            .map_err(|_| Status::invalid_argument("invalid memory_id"))?;
        let memory = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .get_memory(id, &scope.organization_id, &scope.workspace_id)
            .map_err(internal_status)?
            .ok_or_else(|| Status::not_found("memory not found in this workspace"))?;
        self.audit(&principal, &scope, "memory.read", &memory.id.to_string())?;
        Ok(Response::new(memory_proto(memory)))
    }

    async fn retrieve_memories(
        &self,
        request: Request<RetrieveRequest>,
    ) -> Result<Response<MemoryRetrievalResponse>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Reader)?;
        self.enforce_guardrail(&principal, &scope, GuardrailAction::MemoryRead, "memories")?;
        let input = request.into_inner();
        let results = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .retrieve(
                &scope.organization_id,
                &scope.workspace_id,
                &input.query,
                usize::try_from(input.limit.clamp(1, 50)).unwrap_or(50),
            )
            .map_err(internal_status)?;
        self.audit(&principal, &scope, "memory.retrieve", "workspace")?;
        Ok(Response::new(MemoryRetrievalResponse {
            query: input.query,
            results: results.into_iter().map(memory_proto).collect(),
        }))
    }

    async fn retrieve_documents(
        &self,
        request: Request<RetrieveRequest>,
    ) -> Result<Response<DocumentRetrievalResponse>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Reader)?;
        self.enforce_guardrail(
            &principal,
            &scope,
            GuardrailAction::ContextRead,
            "documents",
        )?;
        let input = request.into_inner();
        let results = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .retrieve_chunks(
                &scope.organization_id,
                &scope.workspace_id,
                &input.query,
                usize::try_from(input.limit.clamp(1, 50)).unwrap_or(50),
            )
            .map_err(internal_status)?;
        self.audit(&principal, &scope, "document.retrieve", "workspace")?;
        Ok(Response::new(DocumentRetrievalResponse {
            query: input.query,
            results: results.into_iter().map(chunk_proto).collect(),
            retrieved_content_is_untrusted: true,
        }))
    }

    async fn compile_context_package(
        &self,
        request: Request<ContextPackageRequest>,
    ) -> Result<Response<api::ContextPackage>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Reader)?;
        self.enforce_guardrail(
            &principal,
            &scope,
            GuardrailAction::ContextRead,
            "context-package",
        )?;
        let input = request.into_inner();
        let package = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .compile_context_package(
                &principal,
                &scope.workspace_id,
                &input.query,
                usize::try_from(input.token_budget).unwrap_or(usize::MAX),
                usize::try_from(input.limit.clamp(1, 50)).unwrap_or(50),
            )
            .map_err(internal_status)?;
        self.audit(&principal, &scope, "context.package.read", "workspace")?;
        Ok(Response::new(context_package_proto(package)))
    }

    async fn create_working_session(
        &self,
        request: Request<CreateWorkingSessionRequest>,
    ) -> Result<Response<api::WorkingSession>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Writer)?;
        let input = request.into_inner();
        let session = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .create_working_session(NewWorkingSession {
                organization_id: scope.organization_id.clone(),
                workspace_id: scope.workspace_id.clone(),
                created_by: principal.id,
                ttl_ms: input.ttl_ms.map(u128::from),
            })
            .map_err(internal_status)?;
        self.audit(
            &principal,
            &scope,
            "session.create",
            &session.id.to_string(),
        )?;
        Ok(Response::new(session_proto(session)))
    }

    async fn get_working_session(
        &self,
        request: Request<GetWorkingSessionRequest>,
    ) -> Result<Response<api::WorkingSession>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Reader)?;
        self.enforce_guardrail(
            &principal,
            &scope,
            GuardrailAction::MemoryRead,
            "working-session",
        )?;
        let input = request.into_inner();
        let id = Uuid::parse_str(&input.session_id)
            .map_err(|_| Status::invalid_argument("invalid session_id"))?;
        let session = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .get_working_session(
                id,
                &scope.organization_id,
                &scope.workspace_id,
                principal.id,
            )
            .map_err(internal_status)?
            .ok_or_else(|| Status::not_found("working session not found"))?;
        self.audit(&principal, &scope, "session.read", &session.id.to_string())?;
        Ok(Response::new(session_proto(session)))
    }

    async fn append_working_memory(
        &self,
        request: Request<AppendWorkingMemoryRequest>,
    ) -> Result<Response<api::WorkingMemoryEntry>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Writer)?;
        let input = request.into_inner();
        let session_id = Uuid::parse_str(&input.session_id)
            .map_err(|_| Status::invalid_argument("invalid session_id"))?;
        let kind = working_kind(&input.kind)?;
        let entry = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .append_working_memory(
                session_id,
                &scope.organization_id,
                &scope.workspace_id,
                principal.id,
                NewWorkingMemoryEntry {
                    kind,
                    content: input.content,
                    created_by: principal.id,
                },
            )
            .map_err(internal_status)?
            .ok_or_else(|| Status::not_found("working session not found"))?;
        self.audit(
            &principal,
            &scope,
            "session.entry.append",
            &entry.id.to_string(),
        )?;
        Ok(Response::new(entry_proto(entry)))
    }

    async fn update_working_summary(
        &self,
        request: Request<UpdateWorkingSummaryRequest>,
    ) -> Result<Response<api::WorkingSession>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Writer)?;
        let input = request.into_inner();
        let session_id = Uuid::parse_str(&input.session_id)
            .map_err(|_| Status::invalid_argument("invalid session_id"))?;
        let session = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .update_working_summary(
                session_id,
                &scope.organization_id,
                &scope.workspace_id,
                principal.id,
                input.content,
            )
            .map_err(internal_status)?
            .ok_or_else(|| Status::not_found("working session not found"))?;
        self.audit(
            &principal,
            &scope,
            "session.summary.update",
            &session.id.to_string(),
        )?;
        Ok(Response::new(session_proto(session)))
    }

    async fn promote_working_memory(
        &self,
        request: Request<PromoteWorkingMemoryRequest>,
    ) -> Result<Response<api::Memory>, Status> {
        let scope = required_scope(request.get_ref().scope.as_ref())?;
        let principal = self.authorize(&request, &scope, Role::Writer)?;
        let input = request.into_inner();
        let session_id = Uuid::parse_str(&input.session_id)
            .map_err(|_| Status::invalid_argument("invalid session_id"))?;
        let entry_id = Uuid::parse_str(&input.entry_id)
            .map_err(|_| Status::invalid_argument("invalid entry_id"))?;
        let memory = self
            .state
            .store
            .lock()
            .map_err(|_| Status::internal("storage lock poisoned"))?
            .promote_working_memory_with_limits(
                session_id,
                entry_id,
                &scope.organization_id,
                &scope.workspace_id,
                principal.id,
                input.source,
                input.confidence.unwrap_or(1.0),
                input.expires_at_unix_ms.map(u128::from),
                self.state.limits,
            )
            .map_err(internal_status)?
            .ok_or_else(|| Status::not_found("working session not found"))?;
        self.audit(
            &principal,
            &scope,
            "session.entry.promote",
            &memory.id.to_string(),
        )?;
        Ok(Response::new(memory_proto(memory)))
    }
}

fn required_scope(scope: Option<&Scope>) -> Result<Scope, Status> {
    let scope = scope
        .cloned()
        .ok_or_else(|| Status::invalid_argument("scope is required"))?;
    if scope.organization_id.trim().is_empty() || scope.workspace_id.trim().is_empty() {
        return Err(Status::invalid_argument(
            "organization_id and workspace_id are required",
        ));
    }
    Ok(scope)
}

fn internal_status(error: anyhow::Error) -> Status {
    tracing::warn!(error = %error, "gRPC core operation failed");
    Status::internal("Hangar core operation failed")
}

fn memory_proto(memory: Memory) -> api::Memory {
    api::Memory {
        id: memory.id.to_string(),
        organization_id: memory.organization_id,
        workspace_id: memory.workspace_id,
        content: memory.content,
        source: memory.source,
        confidence: memory.confidence,
        lifecycle: match memory.lifecycle {
            MemoryLifecycle::Proposed => "proposed",
            MemoryLifecycle::Validated => "validated",
            MemoryLifecycle::Published => "published",
            MemoryLifecycle::Superseded => "superseded",
            MemoryLifecycle::Expired => "expired",
        }
        .into(),
        version: u64::from(memory.version),
        created_at_unix_ms: timestamp_u64(memory.created_at_unix_ms),
        updated_at_unix_ms: timestamp_u64(memory.updated_at_unix_ms),
        expires_at_unix_ms: memory.expires_at_unix_ms.map(timestamp_u64),
        superseded_by: memory.superseded_by.map(|id| id.to_string()),
        created_by: memory.created_by.to_string(),
        content_sha256: memory.content_sha256,
        retention: match memory.retention {
            MemoryRetention::Indefinite => "indefinite",
            MemoryRetention::ExpireAt => "expire_at",
        }
        .into(),
        provenance: Some(match memory.provenance {
            MemoryProvenance::Direct => api::MemoryProvenance {
                kind: "direct".into(),
                session_id: None,
                entry_id: None,
                entry_sha256: None,
                session_created_by: None,
            },
            MemoryProvenance::SessionPromotion {
                session_id,
                entry_id,
                entry_sha256,
                session_created_by,
            } => api::MemoryProvenance {
                kind: "session_promotion".into(),
                session_id: Some(session_id.to_string()),
                entry_id: Some(entry_id.to_string()),
                entry_sha256: Some(entry_sha256),
                session_created_by: Some(session_created_by.to_string()),
            },
        }),
    }
}

fn chunk_proto(chunk: RetrievedChunk) -> api::RetrievedChunk {
    api::RetrievedChunk {
        document_id: chunk.document_id.to_string(),
        document_name: chunk.document_name,
        source: chunk.source.unwrap_or_default(),
        ordinal: u32::try_from(chunk.ordinal).unwrap_or(u32::MAX),
        content: chunk.content,
        score: chunk.score,
        vector_score: chunk.vector_score,
        graph_score: chunk.graph_score,
        graph_hops: chunk
            .graph_hops
            .map(|hops| u32::try_from(hops).unwrap_or(u32::MAX)),
        final_score: chunk.final_score,
        embedding_provider: chunk.embedding_provider.unwrap_or_default().into(),
        embedding_model_revision: chunk.embedding_model_revision.unwrap_or_default().into(),
    }
}

fn context_package_proto(package: ContextPackage) -> api::ContextPackage {
    api::ContextPackage {
        organization_id: package.organization_id,
        workspace_id: package.workspace_id,
        query: package.query,
        token_budget: u32::try_from(package.token_budget).unwrap_or(u32::MAX),
        estimated_tokens: u32::try_from(package.estimated_tokens).unwrap_or(u32::MAX),
        items: package
            .items
            .into_iter()
            .map(|item| api::ContextItem {
                content: item.content,
                score: u64::try_from(item.score).unwrap_or(u64::MAX),
                estimated_tokens: u32::try_from(item.estimated_tokens).unwrap_or(u32::MAX),
                untrusted: item.untrusted,
                evidence: Some(api::ContextEvidence {
                    memory_id: item.evidence.memory_id.to_string(),
                    source_workspace_id: item.evidence.source_workspace_id,
                    source: item.evidence.source,
                    content_sha256: item.evidence.content_sha256,
                    memory_version: u64::from(item.evidence.memory_version),
                    share_id: item.evidence.share_id.map(|id| id.to_string()),
                }),
            })
            .collect(),
        policy_notice: package.policy_notice.into(),
    }
}

fn session_proto(session: WorkingSession) -> api::WorkingSession {
    api::WorkingSession {
        id: session.id.to_string(),
        organization_id: session.organization_id,
        workspace_id: session.workspace_id,
        created_by: session.created_by.to_string(),
        created_at_unix_ms: timestamp_u64(session.created_at_unix_ms),
        updated_at_unix_ms: timestamp_u64(session.updated_at_unix_ms),
        expires_at_unix_ms: timestamp_u64(session.expires_at_unix_ms),
        summary: session.summary.map(|summary| api::WorkingSessionSummary {
            content: summary.content,
            content_sha256: summary.content_sha256,
            updated_by: summary.updated_by.to_string(),
            updated_at_unix_ms: timestamp_u64(summary.updated_at_unix_ms),
            version: u64::from(summary.version),
        }),
        entries: session.entries.into_iter().map(entry_proto).collect(),
    }
}

fn entry_proto(entry: crate::store::WorkingMemoryEntry) -> api::WorkingMemoryEntry {
    api::WorkingMemoryEntry {
        id: entry.id.to_string(),
        kind: match entry.kind {
            WorkingMemoryKind::Note => "note",
            WorkingMemoryKind::ToolOutput => "tool_output",
            WorkingMemoryKind::Observation => "observation",
        }
        .into(),
        content: entry.content,
        content_sha256: entry.content_sha256,
        created_by: entry.created_by.to_string(),
        created_at_unix_ms: timestamp_u64(entry.created_at_unix_ms),
    }
}

fn working_kind(value: &str) -> Result<WorkingMemoryKind, Status> {
    match value {
        "" | "note" => Ok(WorkingMemoryKind::Note),
        "tool_output" => Ok(WorkingMemoryKind::ToolOutput),
        "observation" => Ok(WorkingMemoryKind::Observation),
        _ => Err(Status::invalid_argument(
            "kind must be note, tool_output, or observation",
        )),
    }
}

fn timestamp_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    #[test]
    fn protobuf_mapping_preserves_memory_provenance_fields() {
        let memory = Memory {
            id: Uuid::nil(),
            organization_id: "acme".into(),
            workspace_id: "payments".into(),
            content: "OIDC".into(),
            source: Some("adr".into()),
            content_sha256: "hash".into(),
            created_by: Uuid::nil(),
            confidence: 0.9,
            lifecycle: MemoryLifecycle::Proposed,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
            expires_at_unix_ms: None,
            superseded_by: None,
            version: 1,
            retention: crate::store::MemoryRetention::Indefinite,
            provenance: crate::store::MemoryProvenance::Direct,
        };
        let mapped = memory_proto(memory);
        assert_eq!(mapped.lifecycle, "proposed");
        assert_eq!(mapped.source.as_deref(), Some("adr"));
    }

    #[test]
    fn scope_is_required() {
        assert!(required_scope(None).is_err());
        let request = Request::new(HealthRequest {});
        assert!(request.metadata().get("authorization").is_none());
    }
}
