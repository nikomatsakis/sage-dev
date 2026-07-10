# Whiteboard implementation sketch

The MVP whiteboard is an active, parent-linked proof tree. It is created for one
actual execution of `GoalQuery::prove` and dropped when that execution ends.
Salsa can memoize and reuse the completed tracked-function result, but an
in-progress whiteboard is never shared across Salsa executions.

Frames use a parent-sensitive key. An exact duplicate under the same parent
shares one future; equal atoms reached under different parents get distinct
frames. This matches the dependency tree used for inductive cycle detection.

The producer of a shared frame is not owned by the first requester. The query
creates an immutable egraph arena root and a query-owned producer scope. The
top-level request runs in one child of that root; every frame owner imports its
canonical key into another child. Frame branches are therefore siblings of all
requester/candidate branches and never become invalid when a requester is
cancelled.

```rust
const MAX_DEPTH: u32 = 64;

struct Whiteboard<'db> {
    state: SolverStateHandle<'db>,
    producer_scope: FrameProducerScopeHandle,
    /// Immutable for the query lifetime. Frame branches are never collapsed
    /// into it.
    arena_root: Version,
    frames: RefCell<Vec<ProofFrame<'db>>>,
    by_key: RefCell<FxHashMap<FrameKey<'db>, FrameId>>,
    next_subscription: Cell<u32>,
}

/// Canonical data includes local crate, input-variable metadata, environment,
/// and atom. `depth` affects overflow and exact frame reuse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AtomKey<'db> {
    query: Stashed<GoalQueryData<'db, Atom<'db>>>,
    depth: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FrameKey<'db> {
    atom: AtomKey<'db>,
    parent: Option<FrameId>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct FrameId(u32);

struct ProofFrame<'db> {
    key: FrameKey<'db>,
    result_stash: Stash,
    state: FrameState<'db>,
    /// A subscription exists from lookup until that `ProofFuture` becomes
    /// ready or is dropped. `None` means it has not been polled since its last
    /// wake; `Some` stores the current real task waker.
    subscribers: FxHashMap<SubscriptionId, Option<Waker>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct SubscriptionId(u32);

enum FrameState<'db> {
    /// The lookup returned a `NewFrameOwner` which has not started its task yet.
    Starting,
    Running { producer: ProducerHandle },
    /// The last subscriber disappeared. The producer is being drained.
    Cancelling { producer: Option<ProducerHandle> },
    Ready(Stashed<QueryResult<'db>>),
    /// Cancellation cleanup completed. This frame is retained only so old
    /// `FrameId` values are never reinterpreted.
    Abandoned,
}

struct Cycle;

enum Lookup<'wb, 'db> {
    Existing(ProofFuture<'wb, 'db>),
    Created {
        /// Must be started synchronously or its Drop path abandons the frame.
        owner: NewFrameOwner<'wb, 'db>,
        future: ProofFuture<'wb, 'db>,
    },
}

struct ProofFuture<'wb, 'db> {
    whiteboard: &'wb Whiteboard<'db>,
    frame: FrameId,
    subscription: Option<SubscriptionId>,
}

struct NewFrameOwner<'wb, 'db> {
    whiteboard: &'wb Whiteboard<'db>,
    frame: FrameId,
    armed: bool,
}
```

The `Created`/`Existing` distinction gives exactly one `NewFrameOwner` token the
right to start work. That token transfers ownership to the query's producer
scope; it does not give the creating candidate ownership of the task or frame
version. Every requester, including the creator, observes the result only
through its own subscribed future.

## Lookup and cycle detection

```rust
impl<'db> Whiteboard<'db> {
    fn install_producer(&self, frame: FrameId, producer: ProducerHandle) {
        let cancel_immediately = {
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[frame.index()];
            match frame.state {
                FrameState::Starting => {
                    frame.state = FrameState::Running {
                        producer: producer.clone(),
                    };
                    false
                }
                FrameState::Cancelling { producer: None } => {
                    frame.state = FrameState::Cancelling {
                        producer: Some(producer.clone()),
                    };
                    true
                }
                _ => panic!("a frame producer is installed exactly once"),
            }
        };
        if cancel_immediately {
            self.producer_scope.request_cancel(producer);
        }
    }

    fn lookup(
        &self,
        atom: AtomKey<'db>,
        parent: Option<FrameId>,
    ) -> Result<Lookup<'_, 'db>, Cycle> {
        debug_assert!(atom.depth < MAX_DEPTH);

        // Depth is intentionally ignored for cycle identity. Recursive calls
        // increment it, so including it would hide every actual cycle.
        let mut cursor = parent;
        while let Some(frame_id) = cursor {
            let frames = self.frames.borrow();
            let frame = &frames[frame_id.index()];
            debug_assert!(!matches!(frame.state, FrameState::Abandoned));
            if frame.key.atom.query == atom.query {
                return Err(Cycle);
            }
            cursor = frame.key.parent;
        }

        let key = FrameKey { atom, parent };
        if let Some(&frame) = self.by_key.borrow().get(&key) {
            // Cancelling/abandoned frames are removed from `by_key` before
            // cancellation is requested, so they cannot be resubscribed.
            return Ok(Lookup::Existing(self.subscribe(frame)));
        }

        let frame = {
            let mut frames = self.frames.borrow_mut();
            let id = FrameId(frames.len() as u32);
            frames.push(ProofFrame {
                key: key.clone(),
                result_stash: Stash::new(),
                state: FrameState::Starting,
                subscribers: FxHashMap::default(),
            });
            id
        };
        self.by_key.borrow_mut().insert(key, frame);

        Ok(Lookup::Created {
            owner: NewFrameOwner {
                whiteboard: self,
                frame,
                armed: true,
            },
            future: self.subscribe(frame),
        })
    }

    fn subscribe(&self, frame: FrameId) -> ProofFuture<'_, 'db> {
        let id = SubscriptionId(self.next_subscription.get());
        self.next_subscription.set(id.0 + 1);
        self.frames.borrow_mut()[frame.index()]
            .subscribers
            .insert(id, None);
        ProofFuture {
            whiteboard: self,
            frame,
            subscription: Some(id),
        }
    }
}
```

`prove_atom` enforces overflow before lookup: a request with `depth >= 64`
returns `Maybe` directly and creates no frame. A matching ancestor is `No`
because every MVP clause is inductive. Auto traits and other coinductive rules
are deferred rather than special-cased here.

Completed frames remain in `by_key` until the whiteboard is dropped, so a later
exact lookup receives an immediately-ready future. When the last subscriber to
an incomplete frame disappears, its key is removed *before* cancellation is
requested. The old `FrameId` remains as an abandoned tombstone after cleanup,
while a later lookup of the same `FrameKey` may create a new producer.

## Subscriptions, real wakeups, and last-subscriber cancellation

```rust
impl<'wb, 'db> Future for ProofFuture<'wb, 'db> {
    type Output = Stashed<QueryResult<'db>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut frames = this.whiteboard.frames.borrow_mut();
        let frame = &mut frames[this.frame.index()];
        let subscription = this
            .subscription
            .expect("a completed future must not be polled again");

        match &frame.state {
            FrameState::Ready(result) => {
                let result = result.clone();
                frame.subscribers.remove(&subscription);
                this.subscription = None;
                Poll::Ready(result)
            }
            FrameState::Starting | FrameState::Running { .. } => {
                // Re-polling replaces this subscription's waker instead of
                // appending another logical waiter.
                *frame.subscribers
                    .get_mut(&subscription)
                    .expect("live future has a subscription") =
                    Some(cx.waker().clone());
                Poll::Pending
            }
            FrameState::Cancelling { .. } | FrameState::Abandoned => {
                unreachable!("a frame with a live subscriber cannot be abandoned")
            }
        }
    }
}

impl Drop for ProofFuture<'_, '_> {
    fn drop(&mut self) {
        if let Some(subscription) = self.subscription.take() {
            self.whiteboard.unsubscribe(self.frame, subscription);
        }
    }
}

impl<'db> Whiteboard<'db> {
    fn unsubscribe(&self, frame: FrameId, subscription: SubscriptionId) {
        let cancellation = {
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[frame.index()];
            assert!(frame.subscribers.remove(&subscription).is_some());

            if !frame.subscribers.is_empty() {
                None
            } else {
                match &frame.state {
                    FrameState::Starting => {
                        self.by_key.borrow_mut().remove(&frame.key);
                        frame.state = FrameState::Cancelling { producer: None };
                        None
                    }
                    FrameState::Running { producer } => {
                        let producer = producer.clone();
                        self.by_key.borrow_mut().remove(&frame.key);
                        frame.state = FrameState::Cancelling {
                            producer: Some(producer.clone()),
                        };
                        Some(producer)
                    }
                    FrameState::Ready(_) => None,
                    FrameState::Cancelling { .. } | FrameState::Abandoned => {
                        unreachable!("cancellation is requested exactly once")
                    }
                }
            }
        };

        // Drop cannot synchronously join an async producer. Requesting
        // cancellation enqueues its cleanup in the query-owned producer scope.
        if let Some(producer) = cancellation {
            self.producer_scope.request_cancel(producer);
        }
    }

    /// Called only after the producer has joined its descendants, stashed a
    /// branch-independent response, and discarded `frame_version`.
    fn finish_after_discard(
        &self,
        frame: FrameId,
        frame_version: Version,
        result: Stashed<QueryResult<'db>>,
    ) {
        debug_assert!(!self.state.egraph().is_live(frame_version));
        let wakers = {
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[frame.index()];
            match frame.state {
                FrameState::Running { .. } => {
                    frame.state = FrameState::Ready(result);
                    frame.subscribers
                        .values_mut()
                        .filter_map(Option::take)
                        .collect::<Vec<_>>()
                }
                FrameState::Cancelling { .. } => {
                    assert!(frame.subscribers.is_empty());
                    frame.state = FrameState::Abandoned;
                    Vec::new()
                }
                _ => panic!("a frame producer completes exactly once"),
            }
        };

        // Do not wake while holding the RefCell borrow: waking may enqueue and
        // promptly poll a task which accesses the whiteboard again.
        for waker in wakers {
            waker.wake();
        }
    }

    fn abandon_after_discard(&self, frame: FrameId, frame_version: Version) {
        debug_assert!(!self.state.egraph().is_live(frame_version));
        let mut frames = self.frames.borrow_mut();
        let frame = &mut frames[frame.index()];
        assert!(frame.subscribers.is_empty());
        assert!(matches!(frame.state, FrameState::Cancelling { .. }));
        frame.state = FrameState::Abandoned;
    }
}
```

These wakers must be real runtime task wakers whose `wake` implementation puts
the corresponding suspended task back on the ready queue. `Waker::noop()`
cannot implement this protocol. Version-bound waits use the same principle:
wake events produced in speculative state remain buffered until that state is
committed, and discarded/cancelled versions never wake consumers with facts
which did not survive.

The whiteboard lives as long as all its query-owned producers, so retained
wakers cannot outlive the scheduler they target. A subscription is allocated
before the future can be polled, which makes an unpolled future count as live
interest too. Dropping one future never cancels a producer needed by another;
dropping the last pending subscription transitions the frame to `Cancelling`,
removes its cache key, and requests asynchronous cleanup exactly once.

## Task and version lifetime

`NewFrameOwner::spawn_canonical` transfers a starting frame to the query-owned
producer scope:

```rust
impl<'wb, 'db> NewFrameOwner<'wb, 'db> {
    fn spawn_canonical<F, Fut>(
        mut self,
        key: AtomKey<'db>,
        depth: u32,
        solve: F,
    )
    where
        F: FnOnce(ProofCtx<'db>, Atom<'db>, &Scope) -> Fut,
        Fut: Future<Output = Stashed<QueryResult<'db>>>,
    {
        let version = self
            .whiteboard
            .state
            .egraph()
            .branch(self.whiteboard.arena_root);

        // Import only the canonical key. No variable, version, or task handle
        // from the requester which happened to create this frame is captured.
        let (frame_cx, atom) = ProofCtx::import_atomic_key(
            self.whiteboard.state.clone(),
            self.whiteboard,
            self.frame,
            depth,
            version,
            key,
        );

        let frame = self.frame;
        let whiteboard = self.whiteboard;
        let producer = whiteboard.producer_scope.spawn(move |owner_scope| async move {
            match owner_scope.run(|nested| solve(frame_cx, atom, nested)).await {
                ScopeOutcome::Completed(response) => {
                    // `run` joined all candidate/nested work. The response no
                    // longer borrows the branch, so discard before wakeup.
                    whiteboard.state.egraph().discard(version);
                    whiteboard.finish_after_discard(frame, version, response);
                }
                ScopeOutcome::Cancelled => {
                    // Cancellation also joins nested work and drops every
                    // nested ProofFuture before the version is discarded.
                    whiteboard.state.egraph().discard(version);
                    whiteboard.abandon_after_discard(frame, version);
                }
            }
        });

        whiteboard.install_producer(frame, producer);
        self.armed = false;
    }
}
```

The real implementation uses guards so a dropped, not-yet-started
`NewFrameOwner` removes its key and coordinates teardown with its already
allocated subscription. `install_producer`, cancellation, and producer
completion are single-assignment transitions; a cancellation requested before
installation prevents the task from starting.

Trait candidates below that owner may run concurrently, each in an explicit
egraph child version. Their task scope guarantees:

1. the owner does not return while candidate futures are live;
2. early unconditional success cancels and joins every sibling;
3. a candidate version is discarded only after its future has stopped;
4. candidate siblings only extract and discard — none is collapsed into their
   common parent;
5. a nested transactional child is collapsed only when it is the sole live
   child of its parent, so no live sibling observes its parent change;
6. the frame version is a direct child of the immutable arena root, never of a
   candidate/requester version, and is itself only extracted and discarded;
7. result publication happens after the producer's nested scope is joined and
   the frame version is no longer live.

If the candidate which created a frame is cancelled, dropping its
`ProofFuture` removes only that candidate's subscription. Another subscriber
keeps the query-owned producer alive, and the creator's candidate version can
be discarded immediately because the producer never referenced it. If all
subscribers disappear, cancellation drains the producer's nested scope before
discarding its frame version. An abandoned frame is not left in `by_key`.

If the whole `GoalQuery::prove` execution is cancelled, its outer scope first
cancels and joins the top-level requester/candidate scope, which drops its
`ProofFuture` subscriptions. It then cancels and drains the query-owned
producer scope; cancellation of those producers drops their nested frame
subscriptions before each frame version is discarded. Only after every
subscription and producer is gone may it discard the top-level request version
and drop the whiteboard/runtime. Normal execution similarly joins completed
work and drains producers made unneeded by losing candidates before the query
scope exits.

## Operational summary

1. Canonicalize the atom and environment, including input roles/universes and
   `LocalCrateSymbol`.
2. If depth is 64, return `Maybe`.
3. Walk parent frames for the same canonical query; a match is an inductive
   cycle and returns `No`.
4. Build the exact `FrameKey { atom, parent }`.
5. Reuse its future or allocate a subscribed future plus one start token.
6. Start a new producer in a canonical frame branch under the immutable arena
   root; never derive it from the creating requester.
7. Pending polls replace their subscription's real waker. Dropping the last
   pending subscription removes the key and requests producer cancellation.
8. On success, join nested work, stash the response, and discard the frame
   version before storing the result and waking remaining subscribers.
9. Instantiate the completed canonical response in a transactional egraph
   child and collapse it only on complete success.
