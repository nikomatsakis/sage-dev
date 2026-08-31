use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{broadcast, mpsc, oneshot};

use crate::protocol::*;

const MAILBOX_CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub struct Provided<T> {
    pub revision_id: RevisionId,
    pub run_id: Option<RunHandle>,
    pub value: T,
}

impl<T> Provided<T> {
    pub fn without_run(revision_id: impl Into<RevisionId>, value: T) -> Self {
        Self {
            revision_id: revision_id.into(),
            run_id: None,
            value,
        }
    }
}

// ANCHOR: semantic_inspector_service_boundary
pub trait InspectionProvider {
    fn current_revision(&self) -> RevisionId;
    fn revision(&mut self) -> Result<Provided<()>, ApiError>;
    fn session(&mut self) -> Result<Provided<Session>, ApiError>;
    fn symbols(&mut self) -> Result<Provided<SymbolIndex>, ApiError>;
    fn symbol(&mut self, path: &str) -> Result<Provided<SelectedSymbol>, ApiError>;
    fn product(&mut self, symbol: &str, product: &str) -> Result<Provided<ProductPage>, ApiError>;
    fn continuation(&mut self, handle: &str) -> Result<Provided<ContinuationValue>, ApiError>;
    fn run(&mut self, handle: &str) -> Result<Provided<RunObservation>, ApiError>;
    fn revisions(&mut self, cursor: Option<&str>) -> Result<Provided<RevisionPage>, ApiError>;
    fn revision_detail(&mut self, revision: &str) -> Result<Provided<RevisionDetail>, ApiError>;
    fn compare(
        &mut self,
        from: &str,
        to: &str,
        symbol: &str,
        product: &str,
    ) -> Result<Provided<RunComparison>, ApiError>;
    fn apply_updates(
        &mut self,
        updates: Vec<FileUpdate>,
    ) -> Result<Provided<RevisionEvent>, ApiError>;
    fn reload_workspace(&mut self, reason: Issue) -> Result<Provided<RevisionEvent>, ApiError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileUpdate {
    pub path: String,
    pub text: String,
}

#[derive(Clone)]
pub struct InspectionClient {
    sender: mpsc::Sender<Message>,
    events: broadcast::Sender<RevisionEvent>,
    request_ids: Arc<AtomicU64>,
}

pub struct ActorReceiver {
    receiver: mpsc::Receiver<Message>,
    events: broadcast::Sender<RevisionEvent>,
    demand_observer: Option<std::sync::mpsc::Sender<ProviderDemand>>,
}
// ANCHOR_END: semantic_inspector_service_boundary

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDemand {
    pub operation: String,
    pub arguments: Vec<String>,
}

impl ProviderDemand {
    fn new(
        operation: impl Into<String>,
        arguments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            operation: operation.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
        }
    }

    pub fn fixture_line(&self) -> String {
        std::iter::once(format!("provider: {}", self.operation))
            .chain(self.arguments.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl ActorReceiver {
    pub fn observe_demands(&mut self, observer: std::sync::mpsc::Sender<ProviderDemand>) {
        self.demand_observer = Some(observer);
    }

    fn record_demand(&self, demand: ProviderDemand) {
        if let Some(observer) = &self.demand_observer {
            let _ = observer.send(demand);
        }
    }
}

enum Message {
    Revision(Reply<()>),
    Session(Reply<Session>),
    Symbols(Reply<SymbolIndex>),
    Symbol {
        path: String,
        reply: Reply<SelectedSymbol>,
    },
    Product {
        symbol: String,
        product: String,
        reply: Reply<ProductPage>,
    },
    Continuation {
        handle: String,
        reply: Reply<ContinuationValue>,
    },
    Run {
        handle: String,
        reply: Reply<RunObservation>,
    },
    Revisions {
        cursor: Option<String>,
        reply: Reply<RevisionPage>,
    },
    RevisionDetail {
        revision: String,
        reply: Reply<RevisionDetail>,
    },
    Compare {
        from: String,
        to: String,
        symbol: String,
        product: String,
        reply: Reply<RunComparison>,
    },
    ApplyUpdates {
        updates: Vec<FileUpdate>,
        reply: Reply<RevisionEvent>,
    },
    ReloadWorkspace {
        reason: Issue,
        reply: Reply<RevisionEvent>,
    },
}

struct Reply<T> {
    request_id: String,
    sender: oneshot::Sender<Result<Response<T>, ErrorResponse>>,
}

impl InspectionClient {
    pub fn channel() -> (Self, ActorReceiver) {
        let (sender, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        let (events, _) = broadcast::channel(MAILBOX_CAPACITY);
        (
            Self {
                sender,
                events: events.clone(),
                request_ids: Arc::new(AtomicU64::new(0)),
            },
            ActorReceiver {
                receiver,
                events: events.clone(),
                demand_observer: None,
            },
        )
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RevisionEvent> {
        self.events.subscribe()
    }

    pub fn publish(&self, event: RevisionEvent) {
        let _ = self.events.send(event);
    }

    pub async fn revision(&self) -> Result<Response<()>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Revision(reply), receiver).await
    }

    pub async fn session(&self) -> Result<Response<Session>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Session(reply), receiver).await
    }

    pub async fn symbols(&self) -> Result<Response<SymbolIndex>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Symbols(reply), receiver).await
    }

    pub async fn symbol(&self, path: String) -> Result<Response<SelectedSymbol>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Symbol { path, reply }, receiver).await
    }

    pub async fn product(
        &self,
        symbol: String,
        product: String,
    ) -> Result<Response<ProductPage>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(
            Message::Product {
                symbol,
                product,
                reply,
            },
            receiver,
        )
        .await
    }

    pub async fn continuation(
        &self,
        handle: String,
    ) -> Result<Response<ContinuationValue>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Continuation { handle, reply }, receiver)
            .await
    }

    pub async fn run(&self, handle: String) -> Result<Response<RunObservation>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Run { handle, reply }, receiver).await
    }

    pub async fn revisions(
        &self,
        cursor: Option<String>,
    ) -> Result<Response<RevisionPage>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::Revisions { cursor, reply }, receiver)
            .await
    }

    pub async fn revision_detail(
        &self,
        revision: String,
    ) -> Result<Response<RevisionDetail>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::RevisionDetail { revision, reply }, receiver)
            .await
    }

    pub async fn compare(
        &self,
        from: String,
        to: String,
        symbol: String,
        product: String,
    ) -> Result<Response<RunComparison>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(
            Message::Compare {
                from,
                to,
                symbol,
                product,
                reply,
            },
            receiver,
        )
        .await
    }

    pub async fn apply_updates(
        &self,
        updates: Vec<FileUpdate>,
    ) -> Result<Response<RevisionEvent>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::ApplyUpdates { updates, reply }, receiver)
            .await
    }

    pub async fn reload_workspace(
        &self,
        reason: Issue,
    ) -> Result<Response<RevisionEvent>, ErrorResponse> {
        let (reply, receiver) = self.reply();
        self.send(Message::ReloadWorkspace { reason, reply }, receiver)
            .await
    }

    fn reply<T>(
        &self,
    ) -> (
        Reply<T>,
        oneshot::Receiver<Result<Response<T>, ErrorResponse>>,
    ) {
        let request_number = self.request_ids.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, receiver) = oneshot::channel();
        (
            Reply {
                request_id: format!("request-{request_number}"),
                sender,
            },
            receiver,
        )
    }

    async fn send<T>(
        &self,
        message: Message,
        receiver: oneshot::Receiver<Result<Response<T>, ErrorResponse>>,
    ) -> Result<Response<T>, ErrorResponse> {
        if self.sender.send(message).await.is_err() {
            return Err(actor_unavailable());
        }
        receiver.await.unwrap_or_else(|_| Err(actor_unavailable()))
    }
}

pub fn serve_actor(mut provider: impl InspectionProvider, mut actor: ActorReceiver) {
    let provider_name = std::any::type_name_of_val(&provider);
    macro_rules! complete_request {
        ($reply:expr, $demand:expr, $operation:expr) => {{
            let started = std::time::Instant::now();
            let demand = $demand;
            let result = $operation;
            let revision = provider.current_revision();
            complete($reply, result, revision, provider_name, &demand, started);
            actor.record_demand(demand);
        }};
    }

    while let Some(message) = actor.receiver.blocking_recv() {
        match message {
            Message::Revision(reply) => complete_request!(
                reply,
                ProviderDemand::new("current-revision", [] as [&str; 0]),
                provider.revision()
            ),
            Message::Session(reply) => complete_request!(
                reply,
                ProviderDemand::new("session", [] as [&str; 0]),
                provider.session()
            ),
            Message::Symbols(reply) => complete_request!(
                reply,
                ProviderDemand::new("local-symbol-index", [] as [&str; 0]),
                provider.symbols()
            ),
            Message::Symbol { path, reply } => {
                complete_request!(
                    reply,
                    ProviderDemand::new("symbol", [path.clone()]),
                    provider.symbol(&path)
                )
            }
            Message::Product {
                symbol,
                product,
                reply,
            } => complete_request!(
                reply,
                ProviderDemand::new("product", [symbol.clone(), product.clone()]),
                provider.product(&symbol, &product)
            ),
            Message::Continuation { handle, reply } => {
                complete_request!(
                    reply,
                    ProviderDemand::new("continuation", [handle.clone()]),
                    provider.continuation(&handle)
                )
            }
            Message::Run { handle, reply } => {
                complete_request!(
                    reply,
                    ProviderDemand::new("run", [handle.clone()]),
                    provider.run(&handle)
                )
            }
            Message::Revisions { cursor, reply } => {
                let arguments = cursor.iter().cloned();
                complete_request!(
                    reply,
                    ProviderDemand::new("revisions", arguments),
                    provider.revisions(cursor.as_deref())
                )
            }
            Message::RevisionDetail { revision, reply } => {
                complete_request!(
                    reply,
                    ProviderDemand::new("revision-detail", [revision.clone()]),
                    provider.revision_detail(&revision)
                )
            }
            Message::Compare {
                from,
                to,
                symbol,
                product,
                reply,
            } => complete_request!(
                reply,
                ProviderDemand::new(
                    "revision-compare",
                    [from.clone(), to.clone(), symbol.clone(), product.clone()],
                ),
                provider.compare(&from, &to, &symbol, &product)
            ),
            Message::ApplyUpdates { updates, reply } => {
                let started = std::time::Instant::now();
                let result = provider.apply_updates(updates);
                if let Ok(provided) = &result {
                    let _ = actor.events.send(provided.value.clone());
                }
                let revision = provider.current_revision();
                complete(
                    reply,
                    result,
                    revision,
                    provider_name,
                    &ProviderDemand::new("apply-updates", [] as [&str; 0]),
                    started,
                );
                actor.record_demand(ProviderDemand::new("apply-updates", [] as [&str; 0]));
            }
            Message::ReloadWorkspace { reason, reply } => {
                let started = std::time::Instant::now();
                let result = provider.reload_workspace(reason);
                if let Ok(provided) = &result {
                    let _ = actor.events.send(provided.value.clone());
                }
                let revision = provider.current_revision();
                complete(
                    reply,
                    result,
                    revision,
                    provider_name,
                    &ProviderDemand::new("reload-workspace", [] as [&str; 0]),
                    started,
                );
                actor.record_demand(ProviderDemand::new("reload-workspace", [] as [&str; 0]));
            }
        }
    }
}

fn complete<T>(
    reply: Reply<T>,
    result: Result<Provided<T>, ApiError>,
    current_revision: RevisionId,
    provider: &str,
    demand: &ProviderDemand,
    started: std::time::Instant,
) {
    let request_id = reply.request_id;
    let (response, status, run_id) = match result {
        Ok(provided) => Ok(Response {
            revision_id: provided.revision_id,
            request_id: request_id.clone(),
            run_id: provided.run_id,
            value: provided.value,
        })
        .map(|response| {
            let run_id = response.run_id.clone();
            (response, "ok", run_id)
        }),
        Err(error) => Err(ErrorResponse {
            revision_id: current_revision.clone(),
            request_id: request_id.clone(),
            run_id: None,
            error,
        })
        .map_err(|response| (response, "error", None)),
    }
    .map_or_else(
        |(response, status, run_id)| (Err(response), status, run_id),
        |(response, status, run_id)| (Ok(response), status, run_id),
    );
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "inspection-request",
            "request_id": request_id,
            "operation": demand.operation,
            "arguments": demand.arguments,
            "provider": provider,
            "revision_id": current_revision,
            "status": status,
            "run_id": run_id,
            "duration_ms": started.elapsed().as_millis(),
        })
    );
    let _ = reply.sender.send(response);
}

fn actor_unavailable() -> ErrorResponse {
    ErrorResponse {
        revision_id: "rev_unknown".to_owned(),
        request_id: "request-unavailable".to_owned(),
        run_id: None,
        error: ApiError::new("actor-unavailable", "the inspection database actor stopped"),
    }
}
