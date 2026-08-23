#![forbid(unsafe_code)]

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

mod store;

use store::{
    BlobReceipt, HangarStore, IssuedApiKey, Memory, MemoryLifecycle, MemoryTransition, NewMemory,
    Principal, Role, hash_token,
};

#[derive(Parser, Debug)]
#[command(
    name = "hangar-server",
    version,
    about = "Embedded-first AI memory server"
)]
struct Args {
    /// Directory containing all durable Hangar state.
    #[arg(long, env = "HANGAR_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    #[arg(long, env = "HANGAR_LISTEN_ADDR", default_value = "127.0.0.1:8080")]
    listen_addr: SocketAddr,

    /// One-time platform administrator token used only to create an organization owner key.
    #[arg(long, env = "HANGAR_BOOTSTRAP_TOKEN")]
    bootstrap_token: String,
}

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<HangarStore>>,
    bootstrap_token_hash: String,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
    storage: &'static str,
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
}

#[derive(Debug, Deserialize)]
struct TransitionMemoryRequest {
    lifecycle: MemoryLifecycle,
    #[serde(default)]
    expires_at_unix_ms: Option<u128>,
    #[serde(default)]
    superseded_by: Option<Uuid>,
}

fn default_limit() -> usize {
    8
}

#[derive(Debug, Serialize)]
struct RetrievalResponse {
    query: String,
    results: Vec<Memory>,
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

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        storage: "embedded",
    })
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
        store.create_memory(NewMemory {
            organization_id: request.organization_id,
            workspace_id: request.workspace_id,
            content: request.content,
            source: request.source,
            created_by: principal.id,
            confidence: request.confidence.unwrap_or(1.0),
        })
    })?;
    with_store(&state, |store| {
        store.audit(&principal, "memory.create", &memory.id.to_string())
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
    let receipt = with_store(&state, |store| {
        store.put_blob(
            &organization_id,
            &workspace_id,
            media_type,
            &body,
            hex::encode(digest.finalize()),
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
        store.issue_api_key(request.organization_id, request.workspace_id, request.role)
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
    Router::new()
        .route("/health", get(health))
        .route("/v1/memories", post(create_memory))
        .route("/v1/memories/{id}", get(get_memory))
        .route("/v1/memories/{id}/lifecycle", post(transition_memory))
        .route("/v1/retrieve", post(retrieve))
        .route("/v1/blobs", post(put_blob))
        .route("/v1/organizations", post(create_organization))
        .route("/v1/api-keys", post(create_api_key))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("hangar_server=info")
        .init();
    let args = Args::parse();
    let store = HangarStore::open(&args.data_dir)?;
    let listener = tokio::net::TcpListener::bind(args.listen_addr).await?;
    info!(address = %args.listen_addr, data_dir = %args.data_dir.display(), "Hangar server started");
    axum::serve(
        listener,
        app(AppState {
            store: Arc::new(Mutex::new(store)),
            bootstrap_token_hash: hash_token(&args.bootstrap_token),
        }),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}
