#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

mod graph;
mod grpc;
mod operations;
mod sharing;
mod store;
mod text;
mod vector;

use sharing::{ContextPackage, MemoryShare, ShareAudience, ShareReviewState, SubjectKind};
use store::{
    AgentSkill, BlobReceipt, Document, ExportBundle, GraphResult, GuardrailAction,
    GuardrailDecision, GuardrailPolicy, GuardrailRule, HangarStore, IngestionJob, IngestionReceipt,
    IssuedApiKey, Memory, MemoryLifecycle, MemoryProvenance, MemoryTransition, NewAgentSkill,
    NewDocument, NewGuardrailPolicy, NewMemory, NewWorkingMemoryEntry, NewWorkingSession,
    OutboxEvent, PolicyLifecycle, Principal, RetrievedChunk, Role, SkillCapabilities,
    SkillLifecycle, WorkingMemoryEntry, WorkingMemoryKind, WorkingSession, WorkspaceLimits,
    WorkspaceUsage, hash_token,
};

#[derive(Parser, Debug)]
#[command(
    name = "hangar-server",
    version,
    about = "Embedded-first AI memory server"
)]
struct Args {
    /// Directory containing all durable Hangar state.
    #[arg(long, env = "HANGAR_DATA_DIR", default_value = "./data", global = true)]
    data_dir: PathBuf,

    #[arg(long, env = "HANGAR_LISTEN_ADDR", default_value = "127.0.0.1:8080")]
    listen_addr: SocketAddr,

    /// Native gRPC listener. Set it to a private interface when only local
    /// adapters should consume it; it uses the same API-key authorization as HTTP.
    #[arg(
        long,
        env = "HANGAR_GRPC_LISTEN_ADDR",
        default_value = "127.0.0.1:50051"
    )]
    grpc_listen_addr: SocketAddr,

    /// One-time platform administrator token used only to create an organization owner key.
    #[arg(long, env = "HANGAR_BOOTSTRAP_TOKEN")]
    bootstrap_token: Option<String>,

    /// Maximum accepted document payload for one ingestion request.
    #[arg(long, env = "HANGAR_MAX_DOCUMENT_BYTES", default_value_t = 1_048_576)]
    max_document_bytes: usize,

    #[arg(
        long,
        env = "HANGAR_MAX_DOCUMENTS_PER_WORKSPACE",
        default_value_t = 10_000
    )]
    max_documents_per_workspace: usize,

    #[arg(
        long,
        env = "HANGAR_MAX_INGESTION_BYTES_PER_WORKSPACE",
        default_value_t = 536_870_912
    )]
    max_ingestion_bytes_per_workspace: usize,

    #[arg(long, env = "HANGAR_MAX_BLOB_BYTES", default_value_t = 8_388_608)]
    max_blob_bytes: usize,

    #[arg(
        long,
        env = "HANGAR_MAX_BLOB_BYTES_PER_WORKSPACE",
        default_value_t = 1_073_741_824
    )]
    max_blob_bytes_per_workspace: usize,

    #[arg(
        long,
        env = "HANGAR_MAX_MEMORIES_PER_WORKSPACE",
        default_value_t = 50_000
    )]
    max_memories_per_workspace: usize,

    #[arg(
        long,
        env = "HANGAR_MAX_MEMORY_BYTES_PER_WORKSPACE",
        default_value_t = 67_108_864
    )]
    max_memory_bytes_per_workspace: usize,

    /// Embedding space used for document vectors. The default is a
    /// compatibility baseline; `local-multilingual-v1` requires a verified,
    /// pre-provisioned model directory.
    #[arg(long, env = "HANGAR_EMBEDDING_PROFILE", default_value = "hashing-v1")]
    embedding_profile: String,

    /// Directory created by `models install-local`. Required only for the
    /// `local-multilingual-v1` profile and never fetched at runtime.
    #[arg(long, env = "HANGAR_LOCAL_MODEL_DIR")]
    local_model_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Explicit local-model provisioning and verification. These commands are
    /// intentionally separate from normal server startup and request handling.
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// Create a verified filesystem backup. The Hangar server must be stopped.
    Backup {
        #[arg(long)]
        destination: PathBuf,
    },
    /// Verify checksums and the canonical database in an existing backup.
    VerifyBackup {
        #[arg(long)]
        source: PathBuf,
    },
    /// Restore a verified backup into a new, non-existent data directory.
    Restore {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ModelsCommand {
    /// Download the pinned multilingual ONNX model once and write a manifest
    /// with checksums for every provisioned artifact.
    InstallLocal {
        /// Empty, dedicated directory where the model cache and manifest are stored.
        #[arg(long)]
        destination: PathBuf,
    },
    /// Verify a previously provisioned model directory without network access.
    VerifyLocal {
        #[arg(long)]
        source: PathBuf,
    },
}

const LOCAL_MODEL_PROVIDER: &str = crate::vector::LOCAL_MULTILINGUAL_V1_PROVIDER;
const LOCAL_MODEL_REVISION: &str = crate::vector::LOCAL_MULTILINGUAL_V1_MODEL_REVISION;
const LOCAL_MODEL_DIMENSIONS: usize = crate::vector::LOCAL_MULTILINGUAL_V1_DIMENSIONS;
const LOCAL_MODEL_MANIFEST: &str = "hangar-local-model-manifest.json";

#[derive(Debug, Serialize, Deserialize)]
struct LocalModelManifest {
    provider: String,
    model_revision: String,
    dimensions: usize,
    files: BTreeMap<String, String>,
}

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<HangarStore>>,
    bootstrap_token_hash: String,
    limits: WorkspaceLimits,
    metrics: Arc<ServerMetrics>,
}

struct ServerMetrics {
    started_at_unix_ms: u128,
    http_requests: AtomicU64,
    http_server_errors: AtomicU64,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
    storage: &'static str,
}

#[derive(Debug, Serialize)]
struct Readiness {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
struct CreateMemoryRequest {
    organization_id: String,
    workspace_id: String,
    content: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    expires_at_unix_ms: Option<u128>,
}

#[derive(Debug, Deserialize)]
struct MemoryScope {
    organization_id: String,
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
struct RetrieveRequest {
    organization_id: String,
    workspace_id: String,
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct CreateOrganizationRequest {
    organization_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyRequest {
    organization_id: String,
    #[serde(default)]
    workspace_id: Option<String>,
    role: Role,
    #[serde(default)]
    subject_kind: SubjectKind,
}

#[derive(Debug, Deserialize)]
struct TransitionMemoryRequest {
    lifecycle: MemoryLifecycle,
    #[serde(default)]
    expires_at_unix_ms: Option<u128>,
    #[serde(default)]
    superseded_by: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct CreateWorkingSessionRequest {
    organization_id: String,
    workspace_id: String,
    #[serde(default)]
    ttl_ms: Option<u128>,
}

#[derive(Debug, Deserialize)]
struct AppendWorkingMemoryRequest {
    #[serde(default = "default_working_memory_kind")]
    kind: WorkingMemoryKind,
    content: String,
}

#[derive(Debug, Deserialize)]
struct UpdateWorkingSummaryRequest {
    content: String,
}

#[derive(Debug, Deserialize)]
struct PromoteWorkingMemoryRequest {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    expires_at_unix_ms: Option<u128>,
}

#[derive(Debug, Deserialize)]
struct IngestDocumentRequest {
    organization_id: String,
    workspace_id: String,
    name: String,
    #[serde(default)]
    source: Option<String>,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChunkRetrievalResponse {
    query: String,
    results: Vec<RetrievedChunk>,
    content_trust: &'static str,
}

#[derive(Debug, Serialize)]
struct VectorRebuildResponse {
    organization_id: String,
    workspace_id: String,
    vectors_indexed: usize,
}

#[derive(Debug, Serialize)]
struct TextRebuildResponse {
    organization_id: String,
    workspace_id: String,
    chunks_indexed: usize,
}

#[derive(Debug, Deserialize)]
struct GraphRetrieveRequest {
    organization_id: String,
    workspace_id: String,
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_graph_hops")]
    max_hops: usize,
}

#[derive(Debug, Serialize)]
struct GraphRetrievalResponse {
    query: String,
    max_hops: usize,
    results: Vec<GraphResult>,
    content_trust: &'static str,
}

#[derive(Debug, Serialize)]
struct GraphRebuildResponse {
    organization_id: String,
    workspace_id: String,
    chunks_indexed: usize,
}

#[derive(Debug, Deserialize)]
struct OutboxQuery {
    organization_id: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    after: Option<Uuid>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Serialize)]
struct OutboxResponse {
    events: Vec<OutboxEvent>,
}

#[derive(Debug, Deserialize)]
struct CreateSkillRequest {
    organization_id: String,
    workspace_id: String,
    name: String,
    description: String,
    content: String,
    capabilities: SkillCapabilities,
}

#[derive(Debug, Deserialize)]
struct TransitionSkillRequest {
    lifecycle: SkillLifecycle,
}

#[derive(Debug, Deserialize)]
struct CreateGuardrailPolicyRequest {
    organization_id: String,
    workspace_id: String,
    name: String,
    rules: Vec<GuardrailRule>,
}

#[derive(Debug, Deserialize)]
struct TransitionGuardrailPolicyRequest {
    lifecycle: PolicyLifecycle,
}

#[derive(Debug, Deserialize)]
struct GuardrailEvaluationRequest {
    organization_id: String,
    workspace_id: String,
    action: GuardrailAction,
    target: String,
}

#[derive(Debug, Serialize)]
struct SkillUseAuthorization {
    skill: AgentSkill,
    decision: GuardrailDecision,
    content_trust: &'static str,
}

#[derive(Debug, Serialize)]
struct SkillReadResponse {
    skill: AgentSkill,
    content_trust: &'static str,
}

#[derive(Debug, Serialize)]
struct SkillCatalogResponse {
    skills: Vec<AgentSkill>,
    content_trust: &'static str,
}

fn default_limit() -> usize {
    8
}

fn default_graph_hops() -> usize {
    2
}

fn default_working_memory_kind() -> WorkingMemoryKind {
    WorkingMemoryKind::Note
}

#[derive(Debug, Serialize)]
struct RetrievalResponse {
    query: String,
    results: Vec<Memory>,
}

#[derive(Debug, Deserialize)]
struct CreateMemoryShareRequest {
    organization_id: String,
    source_workspace_id: String,
    memory_id: Uuid,
    audience: ShareAudience,
    #[serde(default)]
    expires_at_unix_ms: Option<u128>,
}

#[derive(Debug, Deserialize)]
struct ReviewMemoryShareRequest {
    state: ShareReviewState,
    #[serde(default)]
    review_note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContextPackageRequest {
    organization_id: String,
    workspace_id: String,
    query: String,
    token_budget: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

struct AppError {
    status: StatusCode,
    error: anyhow::Error,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self.error, status = %self.status, "request failed");
        (
            self.status,
            Json(ApiError {
                error: self.error.to_string(),
            }),
        )
            .into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: value,
        }
    }
}

type ApiResult<T> = Result<T, AppError>;

fn with_store<T>(
    state: &AppState,
    operation: impl FnOnce(&mut HangarStore) -> anyhow::Result<T>,
) -> ApiResult<T> {
    let mut store = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?;
    operation(&mut store).map_err(AppError::from)
}

fn unauthorized(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::UNAUTHORIZED,
        error: anyhow::anyhow!(message.into()),
    }
}
fn forbidden(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::FORBIDDEN,
        error: anyhow::anyhow!(message.into()),
    }
}

fn enforce_guardrail(
    state: &AppState,
    principal: &Principal,
    workspace_id: &str,
    action: GuardrailAction,
    target: &str,
) -> ApiResult<GuardrailDecision> {
    let decision = with_store(state, |store| {
        store.evaluate_guardrail(
            &principal.organization_id,
            workspace_id,
            principal.role,
            action,
            target,
        )
    })?;
    let audit_action = if decision.allowed {
        "guardrail.decision.allowed"
    } else {
        "guardrail.decision.denied"
    };
    with_store(state, |store| {
        store.audit(
            principal,
            audit_action,
            &format!("{}:{}", decision.action.as_str(), decision.target),
        )
    })?;
    if decision.allowed {
        Ok(decision)
    } else {
        Err(forbidden(format!(
            "guardrail denied request: {}",
            decision.reason
        )))
    }
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        storage: "embedded",
    })
}

async fn readiness(State(state): State<AppState>) -> ApiResult<Json<Readiness>> {
    with_store(&state, |store| store.check_ready())?;
    Ok(Json(Readiness { status: "ready" }))
}

async fn metrics(State(state): State<AppState>) -> ApiResult<Response> {
    let ready = with_store(&state, |store| store.check_ready()).is_ok();
    let started_seconds = state.metrics.started_at_unix_ms / 1_000;
    let body = format!(
        "# HELP hangar_up Whether the embedded Hangar store is ready (1=true).\n\
# TYPE hangar_up gauge\n\
hangar_up {}\n\
# HELP hangar_process_start_time_seconds Unix time when the server started.\n\
# TYPE hangar_process_start_time_seconds gauge\n\
hangar_process_start_time_seconds {}\n\
# HELP hangar_http_requests_total HTTP requests served by this process.\n\
# TYPE hangar_http_requests_total counter\n\
hangar_http_requests_total {}\n\
# HELP hangar_http_server_errors_total HTTP 5xx responses served by this process.\n\
# TYPE hangar_http_server_errors_total counter\n\
hangar_http_server_errors_total {}\n",
        u8::from(ready),
        started_seconds,
        state.metrics.http_requests.load(Ordering::Relaxed),
        state.metrics.http_server_errors.load(Ordering::Relaxed),
    );
    Ok((
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
        .into_response())
}

async fn observe_http_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    state.metrics.http_requests.fetch_add(1, Ordering::Relaxed);
    let response = next.run(request).await;
    if response.status().is_server_error() {
        state
            .metrics
            .http_server_errors
            .fetch_add(1, Ordering::Relaxed);
    }
    response
}

async fn get_workspace_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<WorkspaceUsage>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let usage = with_store(&state, |store| {
        store.workspace_usage(&scope.organization_id, &scope.workspace_id)
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "operations.usage.read", "workspace")
    })?;
    Ok(Json(usage))
}

async fn export_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<ExportBundle>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::Export,
        "workspace-export",
    )?;
    let export = with_store(&state, |store| {
        store.export_workspace(&scope.organization_id, &scope.workspace_id)
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "workspace.export", "workspace")
    })?;
    Ok(Json(export))
}

async fn create_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateMemoryRequest>,
) -> ApiResult<(StatusCode, Json<Memory>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Writer,
    )?;
    let memory = with_store(&state, |store| {
        store.create_memory_with_limits(
            NewMemory {
                organization_id: request.organization_id,
                workspace_id: request.workspace_id,
                content: request.content,
                source: request.source,
                created_by: principal.id,
                confidence: request.confidence.unwrap_or(1.0),
                expires_at_unix_ms: request.expires_at_unix_ms,
                provenance: MemoryProvenance::Direct,
            },
            state.limits,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.create", &memory.id.to_string())
    })?;
    Ok((StatusCode::CREATED, Json(memory)))
}

async fn create_working_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkingSessionRequest>,
) -> ApiResult<(StatusCode, Json<WorkingSession>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Writer,
    )?;
    let session = with_store(&state, |store| {
        store.create_working_session(NewWorkingSession {
            organization_id: request.organization_id,
            workspace_id: request.workspace_id,
            created_by: principal.id,
            ttl_ms: request.ttl_ms,
        })
    })?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "working_session.create",
            &session.id.to_string(),
        )
    })?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn get_working_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<WorkingSession>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::MemoryRead,
        "working-session",
    )?;
    let session = with_store(&state, |store| {
        store.get_working_session(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            principal.id,
        )
    })?
    .context("working session not found in this scope")?;
    with_store(&state, |store| {
        store.audit(&principal, "working_session.read", &session.id.to_string())
    })?;
    Ok(Json(session))
}

async fn append_working_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<AppendWorkingMemoryRequest>,
) -> ApiResult<(StatusCode, Json<WorkingMemoryEntry>)> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Writer,
    )?;
    let entry = with_store(&state, |store| {
        store.append_working_memory(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            principal.id,
            NewWorkingMemoryEntry {
                kind: request.kind,
                content: request.content,
                created_by: principal.id,
            },
        )
    })?
    .context("working session not found in this scope")?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "working_session.entry.append",
            &entry.id.to_string(),
        )
    })?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn update_working_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<UpdateWorkingSummaryRequest>,
) -> ApiResult<Json<WorkingSession>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Writer,
    )?;
    let session = with_store(&state, |store| {
        store.update_working_summary(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            principal.id,
            request.content,
        )
    })?
    .context("working session not found in this scope")?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "working_session.summary.update",
            &session.id.to_string(),
        )
    })?;
    Ok(Json(session))
}

async fn promote_working_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((session_id, entry_id)): Path<(Uuid, Uuid)>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<PromoteWorkingMemoryRequest>,
) -> ApiResult<(StatusCode, Json<Memory>)> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Writer,
    )?;
    let memory = with_store(&state, |store| {
        store.promote_working_memory_with_limits(
            session_id,
            entry_id,
            &scope.organization_id,
            &scope.workspace_id,
            principal.id,
            request.source,
            request.confidence.unwrap_or(1.0),
            request.expires_at_unix_ms,
            state.limits,
        )
    })?
    .context("working session not found in this scope")?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "working_session.entry.promote",
            &memory.id.to_string(),
        )
    })?;
    Ok((StatusCode::CREATED, Json(memory)))
}

async fn transition_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<TransitionMemoryRequest>,
) -> ApiResult<Json<Memory>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let memory = with_store(&state, |store| {
        store.transition_memory(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            MemoryTransition {
                lifecycle: request.lifecycle,
                expires_at_unix_ms: request.expires_at_unix_ms,
                superseded_by: request.superseded_by,
            },
        )
    })?
    .context("memory not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.transition", &memory.id.to_string())
    })?;
    Ok(Json(memory))
}

async fn ingest_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<IngestDocumentRequest>,
) -> ApiResult<(StatusCode, Json<IngestionReceipt>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Writer,
    )?;
    let idempotency_key = headers
        .get("idempotency-key")
        .map(|value| value.to_str().context("invalid Idempotency-Key header"))
        .transpose()?
        .map(str::to_owned);
    let receipt = with_store(&state, |store| {
        store.enqueue_document_with_limits(
            NewDocument {
                organization_id: request.organization_id,
                workspace_id: request.workspace_id,
                name: request.name,
                source: request.source,
                content: request.content,
                created_by: principal.id,
                idempotency_key,
            },
            state.limits,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "ingestion.enqueue", &receipt.job.id.to_string())
    })?;
    Ok((StatusCode::ACCEPTED, Json(receipt)))
}

async fn get_ingestion_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<IngestionJob>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    let job = with_store(&state, |store| {
        store.get_ingestion_job(id, &scope.organization_id, &scope.workspace_id)
    })?
    .context("ingestion job not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "ingestion.read", &job.id.to_string())
    })?;
    Ok(Json(job))
}

async fn retry_ingestion_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<IngestionJob>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let job = with_store(&state, |store| {
        store.retry_dead_letter_ingestion_job(id, &scope.organization_id, &scope.workspace_id)
    })?
    .context("ingestion job not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "ingestion.retry", &job.id.to_string())
    })?;
    Ok(Json(job))
}

async fn get_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<Document>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    let document = with_store(&state, |store| {
        store.get_document(id, &scope.organization_id, &scope.workspace_id)
    })?
    .context("document not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "document.read", &document.id.to_string())
    })?;
    Ok(Json(document))
}

async fn retrieve_document_chunks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RetrieveRequest>,
) -> ApiResult<Json<ChunkRetrievalResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &request.workspace_id,
        GuardrailAction::ContextRead,
        "documents",
    )?;
    let results = with_store(&state, |store| {
        store.retrieve_chunks(
            &request.organization_id,
            &request.workspace_id,
            &request.query,
            request.limit.clamp(1, 50),
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "document.retrieve", "workspace")
    })?;
    Ok(Json(ChunkRetrievalResponse {
        query: request.query,
        results,
        content_trust: "untrusted_data",
    }))
}

async fn rebuild_vector_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(scope): Json<MemoryScope>,
) -> ApiResult<Json<VectorRebuildResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    with_store(&state, |store| {
        store.audit(&principal, "vector.rebuild.started", "workspace")
    })?;
    match with_store(&state, |store| {
        store.rebuild_vector_workspace(&scope.organization_id, &scope.workspace_id)
    }) {
        Ok(vectors_indexed) => {
            with_store(&state, |store| {
                store.audit(&principal, "vector.rebuild.succeeded", "workspace")
            })?;
            Ok(Json(VectorRebuildResponse {
                organization_id: scope.organization_id,
                workspace_id: scope.workspace_id,
                vectors_indexed,
            }))
        }
        Err(error) => {
            let _ = with_store(&state, |store| {
                store.audit(&principal, "vector.rebuild.failed", "workspace")
            });
            Err(error)
        }
    }
}

async fn rebuild_text_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(scope): Json<MemoryScope>,
) -> ApiResult<Json<TextRebuildResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    with_store(&state, |store| {
        store.audit(&principal, "text.rebuild.started", "workspace")
    })?;
    match with_store(&state, |store| {
        store.rebuild_text_workspace(&scope.organization_id, &scope.workspace_id)
    }) {
        Ok(chunks_indexed) => {
            with_store(&state, |store| {
                store.audit(&principal, "text.rebuild.succeeded", "workspace")
            })?;
            Ok(Json(TextRebuildResponse {
                organization_id: scope.organization_id,
                workspace_id: scope.workspace_id,
                chunks_indexed,
            }))
        }
        Err(error) => {
            let _ = with_store(&state, |store| {
                store.audit(&principal, "text.rebuild.failed", "workspace")
            });
            Err(error)
        }
    }
}

async fn retrieve_graph(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GraphRetrieveRequest>,
) -> ApiResult<Json<GraphRetrievalResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &request.workspace_id,
        GuardrailAction::ContextRead,
        "graph",
    )?;
    let max_hops = request.max_hops.clamp(1, 3);
    let results = with_store(&state, |store| {
        store.retrieve_graph(
            &request.organization_id,
            &request.workspace_id,
            &request.query,
            request.limit.clamp(1, 50),
            max_hops,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "graph.retrieve", "workspace")
    })?;
    Ok(Json(GraphRetrievalResponse {
        query: request.query,
        max_hops,
        results,
        content_trust: "untrusted_data",
    }))
}

async fn rebuild_graph_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(scope): Json<MemoryScope>,
) -> ApiResult<Json<GraphRebuildResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    with_store(&state, |store| {
        store.audit(&principal, "graph.rebuild.started", "workspace")
    })?;
    match with_store(&state, |store| {
        store.rebuild_graph_workspace(&scope.organization_id, &scope.workspace_id)
    }) {
        Ok(chunks_indexed) => {
            with_store(&state, |store| {
                store.audit(&principal, "graph.rebuild.succeeded", "workspace")
            })?;
            Ok(Json(GraphRebuildResponse {
                organization_id: scope.organization_id,
                workspace_id: scope.workspace_id,
                chunks_indexed,
            }))
        }
        Err(error) => {
            let _ = with_store(&state, |store| {
                store.audit(&principal, "graph.rebuild.failed", "workspace")
            });
            Err(error)
        }
    }
}

async fn create_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateSkillRequest>,
) -> ApiResult<(StatusCode, Json<AgentSkill>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Writer,
    )?;
    let skill = with_store(&state, |store| {
        store.create_skill(NewAgentSkill {
            organization_id: request.organization_id,
            workspace_id: request.workspace_id,
            name: request.name,
            description: request.description,
            content: request.content,
            capabilities: request.capabilities,
            created_by: principal.id,
        })
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "skill.create", &skill.id.to_string())
    })?;
    Ok((StatusCode::CREATED, Json(skill)))
}

async fn transition_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<TransitionSkillRequest>,
) -> ApiResult<Json<AgentSkill>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let skill = with_store(&state, |store| {
        store.transition_skill(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            request.lifecycle,
        )
    })?
    .context("skill not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "skill.lifecycle.transition",
            &skill.id.to_string(),
        )
    })?;
    Ok(Json(skill))
}

async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<SkillCatalogResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::SkillRead,
        "catalog",
    )?;
    let skills = with_store(&state, |store| {
        store.list_published_skills(&scope.organization_id, &scope.workspace_id)
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "skill.list", "catalog")
    })?;
    Ok(Json(SkillCatalogResponse {
        skills,
        content_trust: "untrusted_data",
    }))
}

async fn get_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<SkillReadResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::SkillRead,
        &id.to_string(),
    )?;
    let skill = with_store(&state, |store| {
        store.get_published_skill(id, &scope.organization_id, &scope.workspace_id)
    })?
    .context("published skill not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "skill.read", &skill.id.to_string())
    })?;
    Ok(Json(SkillReadResponse {
        skill,
        content_trust: "untrusted_data",
    }))
}

async fn authorize_skill_use(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<SkillUseAuthorization>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    let skill = with_store(&state, |store| {
        store.get_published_skill(id, &scope.organization_id, &scope.workspace_id)
    })?
    .context("published skill not found in this workspace")?;
    let decision = enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::SkillUse,
        &skill.name,
    )?;
    with_store(&state, |store| {
        store.audit(&principal, "skill.use.authorized", &skill.id.to_string())
    })?;
    Ok(Json(SkillUseAuthorization {
        skill,
        decision,
        content_trust: "untrusted_data",
    }))
}

async fn create_guardrail_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateGuardrailPolicyRequest>,
) -> ApiResult<(StatusCode, Json<GuardrailPolicy>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Writer,
    )?;
    let policy = with_store(&state, |store| {
        store.create_guardrail_policy(NewGuardrailPolicy {
            organization_id: request.organization_id,
            workspace_id: request.workspace_id,
            name: request.name,
            rules: request.rules,
            created_by: principal.id,
        })
    })?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "guardrail_policy.create",
            &policy.id.to_string(),
        )
    })?;
    Ok((StatusCode::CREATED, Json(policy)))
}

async fn transition_guardrail_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<TransitionGuardrailPolicyRequest>,
) -> ApiResult<Json<GuardrailPolicy>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let policy = with_store(&state, |store| {
        store.transition_guardrail_policy(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            request.lifecycle,
        )
    })?
    .context("guardrail policy not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(
            &principal,
            "guardrail_policy.lifecycle.transition",
            &policy.id.to_string(),
        )
    })?;
    Ok(Json(policy))
}

async fn list_guardrail_policies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<Vec<GuardrailPolicy>>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let policies = with_store(&state, |store| {
        store.list_guardrail_policies(&scope.organization_id, &scope.workspace_id)
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "guardrail_policy.list", "catalog")
    })?;
    Ok(Json(policies))
}

async fn evaluate_guardrail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GuardrailEvaluationRequest>,
) -> ApiResult<Json<GuardrailDecision>> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Reader,
    )?;
    let decision = enforce_guardrail(
        &state,
        &principal,
        &request.workspace_id,
        request.action,
        &request.target,
    )?;
    Ok(Json(decision))
}

async fn list_outbox_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OutboxQuery>,
) -> ApiResult<Json<OutboxResponse>> {
    let workspace_id = query.workspace_id.as_deref().unwrap_or("");
    let principal = authorize(
        &state,
        &headers,
        &query.organization_id,
        workspace_id,
        Role::Owner,
    )?;
    let events = with_store(&state, |store| {
        store.list_outbox_events(
            &query.organization_id,
            query.workspace_id.as_deref(),
            query.after,
            query.limit.clamp(1, 100),
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "outbox.read", "events")
    })?;
    Ok(Json(OutboxResponse { events }))
}

async fn get_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<Memory>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::MemoryRead,
        "memory",
    )?;
    let memory = with_store(&state, |store| {
        store.get_memory(id, &scope.organization_id, &scope.workspace_id)
    })?
    .context("memory not found in this workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.read", &memory.id.to_string())
    })?;
    Ok(Json(memory))
}

async fn retrieve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RetrieveRequest>,
) -> ApiResult<Json<RetrievalResponse>> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &request.workspace_id,
        GuardrailAction::MemoryRead,
        "memory",
    )?;
    let limit = request.limit.clamp(1, 50);
    let results = with_store(&state, |store| {
        store.retrieve(
            &request.organization_id,
            &request.workspace_id,
            &request.query,
            limit,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.retrieve", "workspace")
    })?;
    Ok(Json(RetrievalResponse {
        query: request.query,
        results,
    }))
}

async fn create_memory_share(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateMemoryShareRequest>,
) -> ApiResult<(StatusCode, Json<MemoryShare>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.source_workspace_id,
        Role::Writer,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &request.source_workspace_id,
        GuardrailAction::MemoryShare,
        "memory-share",
    )?;
    let share = with_store(&state, |store| {
        store.propose_memory_share(
            &request.organization_id,
            &request.source_workspace_id,
            request.memory_id,
            request.audience,
            principal.id,
            request.expires_at_unix_ms,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.share.propose", &share.id.to_string())
    })?;
    Ok((StatusCode::CREATED, Json(share)))
}

async fn list_memory_shares(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(scope): Query<MemoryScope>,
) -> ApiResult<Json<Vec<MemoryShare>>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    let shares = with_store(&state, |store| {
        store.list_memory_shares(&scope.organization_id, &scope.workspace_id)
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.share.list", "source-workspace")
    })?;
    Ok(Json(shares))
}

async fn review_memory_share(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(scope): Query<MemoryScope>,
    Json(request): Json<ReviewMemoryShareRequest>,
) -> ApiResult<Json<MemoryShare>> {
    let principal = authorize(
        &state,
        &headers,
        &scope.organization_id,
        &scope.workspace_id,
        Role::Owner,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &scope.workspace_id,
        GuardrailAction::MemoryShare,
        "memory-share",
    )?;
    let share = with_store(&state, |store| {
        store.review_memory_share(
            id,
            &scope.organization_id,
            &scope.workspace_id,
            request.state,
            principal.id,
            request.review_note,
        )
    })?
    .context("memory share not found in source workspace")?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.share.review", &share.id.to_string())
    })?;
    Ok(Json(share))
}

async fn compile_context_package(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ContextPackageRequest>,
) -> ApiResult<Json<ContextPackage>> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        &request.workspace_id,
        Role::Reader,
    )?;
    enforce_guardrail(
        &state,
        &principal,
        &request.workspace_id,
        GuardrailAction::ContextRead,
        "context-package",
    )?;
    let package = with_store(&state, |store| {
        store.compile_context_package(
            &principal,
            &request.workspace_id,
            &request.query,
            request.token_budget,
            request.limit,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "context.package.read", "workspace")
    })?;
    Ok(Json(package))
}

async fn put_blob(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<BlobReceipt>)> {
    let organization_id = header_value(&headers, "x-hangar-organization-id")?;
    let workspace_id = header_value(&headers, "x-hangar-workspace-id")?;
    let principal = authorize(
        &state,
        &headers,
        &organization_id,
        &workspace_id,
        Role::Writer,
    )?;
    let media_type = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    let mut digest = Sha256::new();
    digest.update(&body);
    let sha256 = hex::encode(digest.finalize());
    let receipt = with_store(&state, |store| {
        store.put_blob_with_limits(
            &organization_id,
            &workspace_id,
            media_type,
            &body,
            sha256,
            state.limits,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "blob.create", &receipt.sha256)
    })?;
    Ok((StatusCode::CREATED, Json(receipt)))
}

async fn create_organization(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateOrganizationRequest>,
) -> ApiResult<(StatusCode, Json<IssuedApiKey>)> {
    authorize_bootstrap(&state, &headers)?;
    let key = with_store(&state, |store| {
        store.issue_api_key(request.organization_id, None, Role::Owner)
    })?;
    let principal = Principal {
        id: key.id,
        organization_id: key.organization_id.clone(),
        workspace_id: None,
        role: key.role,
        subject_kind: key.subject_kind,
    };
    with_store(&state, |store| {
        store.audit(&principal, "organization.create", "organization")
    })?;
    Ok((StatusCode::CREATED, Json(key)))
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<(StatusCode, Json<IssuedApiKey>)> {
    let principal = authorize(
        &state,
        &headers,
        &request.organization_id,
        request.workspace_id.as_deref().unwrap_or(""),
        Role::Owner,
    )?;
    if let Some(scope) = &principal.workspace_id {
        if request.workspace_id.as_deref() != Some(scope) {
            return Err(forbidden(
                "a workspace-scoped owner may issue keys only for its own workspace",
            ));
        }
    }
    let key = with_store(&state, |store| {
        store.issue_api_key_for_subject(
            request.organization_id,
            request.workspace_id,
            request.role,
            request.subject_kind,
        )
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "api_key.create", &key.id.to_string())
    })?;
    Ok((StatusCode::CREATED, Json(key)))
}

fn authorize_bootstrap(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let token = bearer_token(headers)?;
    if hash_token(token) == state.bootstrap_token_hash {
        Ok(())
    } else {
        Err(unauthorized("invalid bootstrap token"))
    }
}

fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    organization_id: &str,
    workspace_id: &str,
    role: Role,
) -> ApiResult<Principal> {
    let token = bearer_token(headers)?;
    let principal = with_store(state, |store| store.authenticate(token))?
        .ok_or_else(|| unauthorized("invalid API key"))?;
    if principal.organization_id != organization_id || !principal.role.allows(role) {
        return Err(forbidden("API key is not permitted for this operation"));
    }
    if let Some(scope) = &principal.workspace_id {
        if scope != workspace_id {
            return Err(forbidden("API key is not permitted for this workspace"));
        }
    }
    Ok(principal)
}

fn bearer_token(headers: &HeaderMap) -> ApiResult<&str> {
    let value = headers
        .get("authorization")
        .ok_or_else(|| unauthorized("missing Authorization header"))?
        .to_str()
        .map_err(|_| unauthorized("invalid Authorization header"))?;
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .ok_or_else(|| unauthorized("expected Bearer token"))
}

fn header_value(headers: &HeaderMap, name: &str) -> anyhow::Result<String> {
    headers
        .get(name)
        .context(format!("missing required {name} header"))?
        .to_str()
        .context(format!("invalid {name} header"))
        .map(str::to_owned)
}

fn app(state: AppState) -> Router {
    let max_request_bytes = state
        .limits
        .max_document_bytes
        .max(state.limits.max_blob_bytes);
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readiness))
        .route("/metrics", get(metrics))
        .route("/v1/memories", post(create_memory))
        .route("/v1/memories/{id}", get(get_memory))
        .route("/v1/memories/{id}/lifecycle", post(transition_memory))
        .route("/v1/sessions", post(create_working_session))
        .route("/v1/sessions/{id}", get(get_working_session))
        .route("/v1/sessions/{id}/entries", post(append_working_memory))
        .route("/v1/sessions/{id}/summary", put(update_working_summary))
        .route(
            "/v1/sessions/{session_id}/entries/{entry_id}/promote",
            post(promote_working_memory),
        )
        .route("/v1/documents", post(ingest_document))
        .route("/v1/documents/{id}", get(get_document))
        .route("/v1/ingestion/jobs/{id}", get(get_ingestion_job))
        .route("/v1/ingestion/jobs/{id}/retry", post(retry_ingestion_job))
        .route("/v1/retrieve/documents", post(retrieve_document_chunks))
        .route("/v1/retrieve/graph", post(retrieve_graph))
        .route("/v1/vector-index/rebuild", post(rebuild_vector_workspace))
        .route("/v1/text-index/rebuild", post(rebuild_text_workspace))
        .route("/v1/graph/rebuild", post(rebuild_graph_workspace))
        .route("/v1/outbox/events", get(list_outbox_events))
        .route("/v1/skills", post(create_skill).get(list_skills))
        .route("/v1/skills/{id}", get(get_skill))
        .route("/v1/skills/{id}/lifecycle", post(transition_skill))
        .route("/v1/skills/{id}/authorize-use", post(authorize_skill_use))
        .route(
            "/v1/guardrail-policies",
            post(create_guardrail_policy).get(list_guardrail_policies),
        )
        .route(
            "/v1/guardrail-policies/{id}/lifecycle",
            post(transition_guardrail_policy),
        )
        .route("/v1/guardrails/evaluate", post(evaluate_guardrail))
        .route("/v1/retrieve", post(retrieve))
        .route(
            "/v1/memory-shares",
            get(list_memory_shares).post(create_memory_share),
        )
        .route("/v1/memory-shares/{id}/review", post(review_memory_share))
        .route("/v1/context-packages", post(compile_context_package))
        .route("/v1/operations/usage", get(get_workspace_usage))
        .route("/v1/exports/workspace", get(export_workspace))
        .route("/v1/blobs", post(put_blob))
        .route("/v1/organizations", post(create_organization))
        .route("/v1/api-keys", post(create_api_key))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            observe_http_request,
        ))
        .layer(DefaultBodyLimit::max(max_request_bytes))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("hangar_server=info")
        .init();
    let args = Args::parse();
    if let Some(command) = args.command {
        match command {
            Command::Models { command } => match command {
                ModelsCommand::InstallLocal { destination } => {
                    install_local_model(&destination)?;
                    info!(destination = %destination.display(), "local embedding model installed and verified");
                }
                ModelsCommand::VerifyLocal { source } => {
                    verify_local_model(&source)?;
                    info!(source = %source.display(), "local embedding model verification succeeded");
                }
            },
            Command::Backup { destination } => {
                operations::create_backup(&args.data_dir, &destination)?;
                info!(source = %args.data_dir.display(), destination = %destination.display(), "backup created and verified");
            }
            Command::VerifyBackup { source } => {
                operations::verify_backup(&source)?;
                info!(source = %source.display(), "backup verification succeeded");
            }
            Command::Restore {
                source,
                destination,
            } => {
                operations::restore_backup(&source, &destination)?;
                info!(source = %source.display(), destination = %destination.display(), "backup restored and verified");
            }
        }
        return Ok(());
    }
    let bootstrap_token = args
        .bootstrap_token
        .as_deref()
        .context("HANGAR_BOOTSTRAP_TOKEN is required when serving the API")?;
    let limits = WorkspaceLimits {
        max_document_bytes: args.max_document_bytes,
        max_documents: args.max_documents_per_workspace,
        max_ingestion_bytes: args.max_ingestion_bytes_per_workspace,
        max_blob_bytes: args.max_blob_bytes,
        max_blobs_bytes: args.max_blob_bytes_per_workspace,
        max_memories: args.max_memories_per_workspace,
        max_memory_bytes: args.max_memory_bytes_per_workspace,
    };
    anyhow::ensure!(
        [
            limits.max_document_bytes,
            limits.max_documents,
            limits.max_ingestion_bytes,
            limits.max_blob_bytes,
            limits.max_blobs_bytes,
            limits.max_memories,
            limits.max_memory_bytes,
        ]
        .into_iter()
        .all(|limit| limit > 0),
        "all Hangar operational limits must be greater than zero"
    );
    let embedding_provider = match args.embedding_profile.as_str() {
        "hashing-v1" => crate::vector::EmbeddingProvider::hashing_v1(),
        "local-multilingual-v1" => {
            let source = args.local_model_dir.as_deref().context(
                "HANGAR_LOCAL_MODEL_DIR is required for HANGAR_EMBEDDING_PROFILE=local-multilingual-v1",
            )?;
            verify_local_model(source)?;
            crate::vector::EmbeddingProvider::local_multilingual_v1_from_verified_cache(source)?
        }
        other => anyhow::bail!(
            "unsupported HANGAR_EMBEDDING_PROFILE={other}; supported profiles are hashing-v1 and local-multilingual-v1"
        ),
    };
    let store = HangarStore::open_with_embedding_provider(&args.data_dir, embedding_provider)?;
    let state = AppState {
        store: Arc::new(Mutex::new(store)),
        bootstrap_token_hash: hash_token(bootstrap_token),
        limits,
        metrics: Arc::new(ServerMetrics {
            started_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
            http_requests: AtomicU64::new(0),
            http_server_errors: AtomicU64::new(0),
        }),
    };
    let expired_memories = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?
        .expire_due_memories()?;
    let recovered = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?
        .recover_incomplete_ingestion_jobs()?;
    let removed_vector_temporary_files = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?
        .reconcile_vector_projection()?;
    let removed_text_generations = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?
        .reconcile_text_projection()?;
    let reconciled_graph_workspaces = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?
        .reconcile_graph_projection()?;
    if recovered > 0 {
        info!(recovered, "recovered incomplete ingestion jobs");
    }
    if expired_memories > 0 {
        info!(
            expired_memories,
            "expired durable memories by retention policy"
        );
    }
    if removed_vector_temporary_files > 0 {
        info!(
            removed_vector_temporary_files,
            "removed interrupted vector publications"
        );
    }
    if removed_text_generations > 0 {
        info!(removed_text_generations, "removed stale text generations");
    }
    if reconciled_graph_workspaces > 0 {
        info!(reconciled_graph_workspaces, "reconciled graph workspaces");
    }
    spawn_ingestion_worker(state.clone());
    let grpc_state = state.clone();
    let grpc_listen_addr = args.grpc_listen_addr;
    tokio::spawn(async move {
        let service = grpc::api::hangar_service_server::HangarServiceServer::new(
            grpc::GrpcApi::new(grpc_state),
        );
        if let Err(error) = tonic::transport::Server::builder()
            .add_service(service)
            .serve(grpc_listen_addr)
            .await
        {
            tracing::error!(error = %error, address = %grpc_listen_addr, "Hangar gRPC server stopped");
        }
    });
    let listener = tokio::net::TcpListener::bind(args.listen_addr).await?;
    info!(
        address = %args.listen_addr,
        grpc_address = %args.grpc_listen_addr,
        data_dir = %args.data_dir.display(),
        "Hangar server started"
    );
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn install_local_model(destination: &FsPath) -> anyhow::Result<()> {
    if destination.exists() {
        anyhow::ensure!(
            destination.read_dir()?.next().is_none(),
            "local model destination must be empty: {}",
            destination.display()
        );
    } else {
        fs::create_dir_all(destination)?;
    }

    // This command is the only supported path that may contact the model
    // registry. Server startup and request handling never call it.
    let mut model = fastembed::TextEmbedding::try_new(
        fastembed::InitOptions::new(fastembed::EmbeddingModel::ParaphraseMLMiniLML12V2Q)
            .with_cache_dir(destination.to_path_buf())
            .with_show_download_progress(true),
    )
    .context("downloading the pinned local multilingual ONNX model")?;
    let vector = model
        .embed(
            vec!["Hangar local embedding installation check".to_owned()],
            None,
        )
        .context("running the pinned local embedding model")?;
    anyhow::ensure!(
        vector.len() == 1 && vector[0].len() == LOCAL_MODEL_DIMENSIONS,
        "pinned local model returned unexpected dimensions"
    );
    write_local_model_manifest(destination)?;
    verify_local_model(destination).map(|_| ())
}

fn write_local_model_manifest(root: &FsPath) -> anyhow::Result<()> {
    let files = collect_model_files(root)?;
    anyhow::ensure!(
        !files.is_empty(),
        "local model installation produced no files"
    );
    let manifest = LocalModelManifest {
        provider: LOCAL_MODEL_PROVIDER.to_owned(),
        model_revision: LOCAL_MODEL_REVISION.to_owned(),
        dimensions: LOCAL_MODEL_DIMENSIONS,
        files,
    };
    fs::write(
        root.join(LOCAL_MODEL_MANIFEST),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn verify_local_model(root: &FsPath) -> anyhow::Result<LocalModelManifest> {
    let manifest_path = root.join(LOCAL_MODEL_MANIFEST);
    let manifest: LocalModelManifest =
        serde_json::from_slice(&fs::read(&manifest_path).with_context(|| {
            format!("reading local model manifest {}", manifest_path.display())
        })?)?;
    anyhow::ensure!(
        manifest.provider == LOCAL_MODEL_PROVIDER
            && manifest.model_revision == LOCAL_MODEL_REVISION
            && manifest.dimensions == LOCAL_MODEL_DIMENSIONS,
        "local model manifest does not match the supported profile"
    );
    anyhow::ensure!(
        !manifest.files.is_empty(),
        "local model manifest has no artifacts"
    );
    for (relative, expected_sha256) in &manifest.files {
        let relative = FsPath::new(relative);
        anyhow::ensure!(
            !relative.is_absolute()
                && !relative
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir)),
            "local model manifest contains an unsafe artifact path"
        );
        let artifact = root.join(relative);
        anyhow::ensure!(
            artifact.is_file(),
            "local model artifact is missing: {}",
            artifact.display()
        );
        anyhow::ensure!(
            sha256_file(&artifact)? == *expected_sha256,
            "local model artifact checksum mismatch: {}",
            artifact.display()
        );
    }
    Ok(manifest)
}

fn collect_model_files(root: &FsPath) -> anyhow::Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    collect_model_files_at(root, root, &mut files)?;
    Ok(files)
}

fn collect_model_files_at(
    root: &FsPath,
    directory: &FsPath,
    files: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_model_files_at(root, &path, files)?;
        } else if path.is_file() && entry.file_name() != LOCAL_MODEL_MANIFEST {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, sha256_file(&path)?);
        }
    }
    Ok(())
}

fn sha256_file(path: &FsPath) -> anyhow::Result<String> {
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}

#[cfg(test)]
mod local_model_tests {
    use super::*;

    #[test]
    fn local_model_manifest_detects_modified_artifacts() {
        let directory = tempfile::tempdir().expect("create temporary model directory");
        let artifact = directory.path().join("model.onnx");
        fs::write(&artifact, b"known-good-model").expect("write model artifact");

        write_local_model_manifest(directory.path()).expect("write local model manifest");
        verify_local_model(directory.path()).expect("verify intact local model");

        fs::write(&artifact, b"tampered-model").expect("modify model artifact");
        assert!(verify_local_model(directory.path()).is_err());
    }
}

fn spawn_ingestion_worker(state: AppState) {
    tokio::spawn(async move {
        loop {
            let claimed = match with_store(&state, |store| store.claim_next_ingestion_job()) {
                Ok(claimed) => claimed,
                Err(error) => {
                    tracing::error!(error = %error.error, "ingestion worker could not lease a job");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            let Some(claimed) = claimed else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            let job_id = claimed.job.id;
            if let Err(error) =
                with_store(&state, |store| store.process_claimed_ingestion_job(claimed))
            {
                let message = error.error.to_string();
                if let Err(failure) = with_store(&state, |store| {
                    store.fail_claimed_ingestion_job(job_id, &message)
                }) {
                    tracing::error!(error = %failure.error, job_id = %job_id, "ingestion worker could not record job failure");
                } else {
                    tracing::warn!(error = %message, job_id = %job_id, "ingestion job failed");
                }
            }
        }
    });
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}
