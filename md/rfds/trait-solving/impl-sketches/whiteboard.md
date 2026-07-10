# Whiteboard implementation sketch

The MVP whiteboard is an active, parent-linked proof tree created for one
actual execution of `GoalQuery::prove`. Salsa can reuse the completed tracked
result, but an in-progress whiteboard is never shared across executions.

Frames use a parent-sensitive key. An exact duplicate under the same parent
shares one future; equal atoms reached through different parents get distinct
frames. The parent chain is also the inductive cycle stack. Remaining depth is
part of exact reuse but deliberately excluded from cycle identity.

## Isolated producers

Under [D8](../../../design/decisions.md#d8-whiteboard-producers-own-isolated-proof-contexts),
each producer imports its canonical query into a fresh `QueryProofState`: its
own stash, egraph, and root version. Each candidate alternative imports the
same canonical query into another independent proof state and performs head
matching in a local child transaction. Producers and candidates therefore
never borrow a requester's egraph or see its branch-local identities.

The common coordination object is the whiteboard itself, and the only state
published between proof contexts is a validated, branch-independent
`Stashed<QueryResult>`.

```rust
struct Whiteboard<'db> {
    inner: Rc<RefCell<WhiteboardInner<'db>>>,
}

struct WhiteboardInner<'db> {
    next_subscription: u32,
    frames: Vec<ProofFrame<'db>>,
    by_key: FxHashMap<FrameKey<'db>, FrameId>,
    producers: Vec<Option<ProducerFuture<'db>>>,
    ready: Arc<Mutex<ProducerReady>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FrameKey<'db> {
    query: Stashed<GoalQueryData<'db>>,
    parent: Option<FrameId>,
    remaining_depth: u32,
}

struct ProofFrame<'db> {
    query: Stashed<GoalQueryData<'db>>,
    parent: Option<FrameId>,
    remaining_depth: u32,
    state: FrameState<'db>,
    producer_started: bool,
    subscriptions: FxHashMap<SubscriptionId, Option<Waker>>,
}

enum FrameState<'db> {
    Pending,
    Ready(Stashed<QueryResult<'db>>),
    Abandoned,
}
```

Frame IDs are append-only. Cancellation removes the key and marks the old
frame abandoned; a later equivalent lookup allocates a new ID rather than
reinterpreting the tombstone.

## Lookup and subscriptions

For an atomic request at depth `D`:

1. If `D >= MAX_PROOF_DEPTH`, return `Maybe` without a frame.
2. Walk `parent`. If an ancestor has the same canonical query while ignoring
   depth, return inductive `No`.
3. Reuse `by_key[(query, parent, remaining_depth)]`, or append a pending frame.
4. Allocate a subscription immediately, before returning `ProofFuture`.
5. For a new frame, install one producer future in the query-owned producer
   table and enqueue it. Existing requesters only subscribe.

```rust
struct ProofFuture<'db> {
    whiteboard: Option<Whiteboard<'db>>,
    frame: FrameId,
    subscription: SubscriptionId,
}

impl Future for ProofFuture<'_> {
    type Output = Stashed<QueryResult<'_>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match frame.state {
            FrameState::Ready(result) => {
                self.whiteboard = None;
                Poll::Ready(result.clone())
            }
            FrameState::Pending => {
                frame.subscriptions[self.subscription] = Some(cx.waker().clone());
                Poll::Pending
            }
            FrameState::Abandoned => panic!("stale frame access"),
        }
    }
}
```

Dropping a pending future removes only its subscription. If subscribers remain,
the producer is unaffected, including when the dropped subscriber originally
created the frame. If the last subscription disappears, cleanup proceeds in
this order:

1. remove the frame key so a later lookup can start fresh work;
2. mark the frame abandoned and take its producer handle;
3. drop the producer outside the whiteboard borrow;
4. producer drop cancels its scoped candidates, which drop their nested
   `ProofFuture`s and recursively unregister subscriptions;
5. finally drop every producer-owned proof context.

This synchronous drop chain is the whiteboard's cancellation-and-join path in
the single-threaded cooperative executor.

## Producer driving and completion

The query driver polls the root request and a real-waker ready queue of producer
futures. A producer may await nested atomic frames. Completing that nested frame
wakes the candidate task; its scoped-task waker then wakes the owning producer.
This permits sibling candidates to register the same nested request before the
shared producer completes.

Candidate tasks own independent proof states. A completed candidate extracts a
canonical answer and drops its child transaction. An exact unconditional
answer cancels remaining scoped candidates; otherwise all answers are retained
for order-independent merging.

On producer completion:

1. all candidate tasks have completed or been cancelled;
2. the producer-owned proof state has yielded a canonical stashed response;
3. store that response exactly once and take every subscriber waker;
4. drop the completed producer future and its proof context;
5. wake subscribers.

Completed entries remain reusable until query teardown. Before returning from
`GoalQuery::prove`, the driver asserts that no frame is pending and no producer
handle remains. Double completion, polling an abandoned frame, stale frame
access, and a pending proof with no runnable producer are internal errors.

## Required lifecycle properties

- Same-parent duplicates share one in-progress producer; different-parent
  requests do not.
- An ancestor repeat is `No`; equal completed non-ancestors are ordinary
  reusable frames rather than cycles.
- Cancelling one subscriber cannot cancel work needed by another.
- Cancelling the last subscriber removes the key before recursively draining
  nested work.
- Producer and candidate proof contexts contain no requester branch identity.
- A result becomes visible only after extraction has removed raw egraph IDs.
- Query completion leaves no live producer, subscription, or proof context.
