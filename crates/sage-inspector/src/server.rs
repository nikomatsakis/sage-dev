use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::get;
use futures_util::stream;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde::Serialize;
use tokio::net::TcpListener;

use crate::actor::InspectionClient;
use crate::protocol::*;

#[derive(Clone, Copy, Debug)]
pub struct ServerOptions {
    pub port: u16,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { port: 2442 }
    }
}

impl ServerOptions {
    pub fn address(self) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.port)
    }
}

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct WebAssets;

#[derive(Clone)]
struct AppState {
    client: InspectionClient,
}

pub fn router(client: InspectionClient) -> Router {
    Router::new()
        .route("/api/v1/revision", get(revision))
        .route("/api/v1/session", get(session))
        .route("/api/v1/symbols", get(symbols))
        .route("/api/v1/symbol", get(symbol))
        .route("/api/v1/product", get(product))
        .route("/api/v1/continuations/{handle}", get(continuation))
        .route("/api/v1/runs/{handle}", get(run))
        .route("/api/v1/events", get(events))
        .route("/api/v1/revisions", get(revisions))
        .route("/api/v1/revisions/compare", get(compare))
        .route("/api/v1/revisions/{revision}", get(revision_detail))
        .fallback(get(asset))
        .with_state(AppState { client })
}

pub async fn bind(options: ServerOptions) -> std::io::Result<(SocketAddr, TcpListener)> {
    let listener = TcpListener::bind(options.address()).await?;
    let address = listener.local_addr()?;
    Ok((address, listener))
}

pub async fn run_server(listener: TcpListener, client: InspectionClient) -> std::io::Result<()> {
    axum::serve(listener, router(client)).await
}

async fn revision(State(state): State<AppState>) -> ApiResponse<()> {
    ApiResponse(state.client.revision().await)
}

async fn session(State(state): State<AppState>) -> ApiResponse<Session> {
    ApiResponse(state.client.session().await)
}

async fn symbols(State(state): State<AppState>) -> ApiResponse<SymbolIndex> {
    ApiResponse(state.client.symbols().await)
}

#[derive(Deserialize)]
struct SymbolQuery {
    path: String,
}

async fn symbol(
    State(state): State<AppState>,
    query: Result<Query<SymbolQuery>, QueryRejection>,
) -> ApiResponse<SelectedSymbol> {
    let Ok(Query(query)) = query else {
        return protocol_error(
            &state.client,
            "invalid-request",
            "missing or invalid symbol path",
        )
        .await;
    };
    ApiResponse(state.client.symbol(query.path).await)
}

#[derive(Deserialize)]
struct ProductQuery {
    symbol: String,
    product: String,
}

async fn product(
    State(state): State<AppState>,
    query: Result<Query<ProductQuery>, QueryRejection>,
) -> ApiResponse<ProductPage> {
    let Ok(Query(query)) = query else {
        return protocol_error(
            &state.client,
            "invalid-request",
            "missing or invalid symbol/product query",
        )
        .await;
    };
    ApiResponse(state.client.product(query.symbol, query.product).await)
}

async fn continuation(
    State(state): State<AppState>,
    Path(handle): Path<String>,
) -> ApiResponse<ContinuationValue> {
    ApiResponse(state.client.continuation(handle).await)
}

async fn run(
    State(state): State<AppState>,
    Path(handle): Path<String>,
) -> ApiResponse<RunObservation> {
    ApiResponse(state.client.run(handle).await)
}

#[derive(Default, Deserialize)]
struct RevisionsQuery {
    cursor: Option<String>,
}

async fn revisions(
    State(state): State<AppState>,
    query: Result<Query<RevisionsQuery>, QueryRejection>,
) -> ApiResponse<RevisionPage> {
    let Ok(Query(query)) = query else {
        return protocol_error(&state.client, "invalid-request", "invalid revision cursor").await;
    };
    ApiResponse(state.client.revisions(query.cursor).await)
}

async fn revision_detail(
    State(state): State<AppState>,
    Path(revision): Path<String>,
) -> ApiResponse<RevisionDetail> {
    ApiResponse(state.client.revision_detail(revision).await)
}

#[derive(Deserialize)]
struct CompareQuery {
    from: String,
    to: String,
    symbol: String,
    product: String,
}

async fn compare(
    State(state): State<AppState>,
    query: Result<Query<CompareQuery>, QueryRejection>,
) -> ApiResponse<RunComparison> {
    let Ok(Query(query)) = query else {
        return protocol_error(
            &state.client,
            "invalid-request",
            "missing or invalid revision comparison query",
        )
        .await;
    };
    ApiResponse(
        state
            .client
            .compare(query.from, query.to, query.symbol, query.product)
            .await,
    )
}

async fn protocol_error<T>(client: &InspectionClient, code: &str, message: &str) -> ApiResponse<T> {
    ApiResponse(match client.revision().await {
        Ok(revision) => Err(ErrorResponse {
            revision_id: revision.revision_id,
            request_id: revision.request_id,
            run_id: None,
            error: ApiError::new(code, message),
        }),
        Err(error) => Err(error),
    })
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.client.subscribe();
    let events = stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(revision_event) => {
                    let (name, value) = match revision_event {
                        RevisionEvent::RevisionAdvanced(value) => {
                            ("revision-advanced", serde_json::to_string(&value))
                        }
                        RevisionEvent::WorkspaceReloaded(value) => {
                            ("workspace-reloaded", serde_json::to_string(&value))
                        }
                    };
                    let event = Event::default()
                        .event(name)
                        .data(value.expect("revision event must serialize"));
                    return Some((Ok(event), receiver));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

async fn asset(State(state): State<AppState>, uri: axum::http::Uri) -> HttpResponse {
    let requested = uri.path().trim_start_matches('/');
    if requested.starts_with("api/") {
        return match state.client.revision().await {
            Ok(revision) => ApiResponse::<()>(Err(ErrorResponse {
                revision_id: revision.revision_id,
                request_id: revision.request_id,
                run_id: None,
                error: ApiError::new("not-found", format!("unknown API route `{}`", uri.path())),
            }))
            .into_response(),
            Err(error) => ApiResponse::<()>(Err(error)).into_response(),
        };
    }
    let requested_path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let (resolved_path, asset) = if let Some(asset) = WebAssets::get(requested_path) {
        (requested_path, Some(asset))
    } else if is_navigation_route(requested_path) {
        ("index.html", WebAssets::get("index.html"))
    } else {
        (requested_path, None)
    };
    let Some(asset) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let content_type = mime_guess::from_path(resolved_path)
        .first_or_octet_stream()
        .as_ref()
        .to_owned();
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&content_type).unwrap(),
        )],
        asset.data,
    )
        .into_response()
}

fn is_navigation_route(path: &str) -> bool {
    path.is_empty()
        || path == "index.html"
        || (!path
            .rsplit('/')
            .next()
            .is_some_and(|segment| segment.contains('.')))
}

struct ApiResponse<T>(Result<crate::protocol::Response<T>, ErrorResponse>);

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> HttpResponse {
        let (status, bytes) = match self.0 {
            Ok(response) => (
                StatusCode::OK,
                pretty_json(&response).expect("API response must serialize"),
            ),
            Err(response) => {
                let status = match response.error.code.as_str() {
                    "not-found"
                    | "symbol-not-found"
                    | "product-not-found"
                    | "continuation-not-found"
                    | "run-not-found"
                    | "revision-not-found"
                    | "comparison-value-not-found" => StatusCode::NOT_FOUND,
                    "invalid-request" => StatusCode::BAD_REQUEST,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (
                    status,
                    pretty_json(&response).expect("API error must serialize"),
                )
            }
        };

        (
            status,
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json; charset=utf-8"),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            bytes,
        )
            .into_response()
    }
}

fn pretty_json(value: &impl Serialize) -> serde_json::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
