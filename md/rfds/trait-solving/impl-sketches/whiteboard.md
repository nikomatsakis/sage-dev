# Whiteboard implementation sketch

The MVP whiteboard is an active proof tree, not a cache. It is scoped to one
`GoalQuery::prove` invocation. Completed frames remain in the whiteboard until
that invocation finishes; there is no cross-query reuse.

```rust
struct Whiteboard<'db> {
    frames: RefCell<Vec<ProofFrame<'db>>>,
    active: RefCell<FxHashMap<FrameKey<'db>, FrameId>>,
}

struct AtomKey<'db> {
    atom: Stashed<GoalQueryData<'db, Atom<'db>>>,
    depth: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct FrameId(u32);

struct ProofFrame<'db> {
    key: AtomKey<'db>,
    parent: Option<FrameId>,
    result: Option<Stashed<QueryResult<'db>>>,
}

struct FrameKey<'db> {
    key: AtomKey<'db>,
    parent: Option<FrameId>,
}

struct Cycle;

struct ProofFuture<'wb, 'db> {
    whiteboard: &'wb Whiteboard<'db>,
    frame: FrameId,
}

impl<'db> Whiteboard<'db> {
    fn lookup(
        &self,
        key: AtomKey<'db>,
        parent: Option<FrameId>,
    ) -> Result<(FrameId, ProofFuture<'_, 'db>), Cycle> {
        // Cycle detection is independent of the entries table.
        let mut cursor = parent;
        while let Some(frame_id) = cursor {
            let frame = &self.frames.borrow()[frame_id.index()];
            if frame.key.same_atom_and_env(&key) {
                return Err(Cycle);
            }
            cursor = frame.parent;
        }

        let frame_key = FrameKey { key, parent };

        if let Some(&frame) = self.active.borrow().get(&frame_key) {
            return Ok((
                frame,
                ProofFuture {
                    whiteboard: self,
                    frame,
                },
            ));
        }

        let frame = ProofFrame {
            key,
            parent,
            result: None,
        };
        let frame_id = FrameId(self.frames.borrow().len() as u32);
        self.frames.borrow_mut().push(frame);

        self.active.borrow_mut().insert(frame_key, frame_id);

        Ok((
            frame_id,
            ProofFuture {
                whiteboard: self,
                frame: frame_id,
            },
        ))
    }

    fn finish(&self, frame: FrameId, result: Stashed<QueryResult<'db>>) {
        let mut frames = self.frames.borrow_mut();
        let frame_data = &mut frames[frame.index()];
        debug_assert!(frame_data.result.is_none());
        frame_data.result = Some(result);
    }
}

impl<'wb, 'db> Future for ProofFuture<'wb, 'db> {
    type Output = Stashed<QueryResult<'db>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let frames = self.whiteboard.frames.borrow();
        let frame = &frames[self.frame.index()];

        match frame.result {
            Some(result) => Poll::Ready(result),
            None => {
                // Real code must arrange for `cx.waker()` to be woken by
                // `finish`; this sketch leaves the storage strategy open.
                Poll::Pending
            }
        }
    }
}
```

Cycle handling:

1. Each spawned atomic proof gets a `ProofFrame` with the canonical atom/environment/depth and a parent frame.
2. On lookup:
   - first walk `parent` links; if an ancestor frame has the same canonical atom/environment, return `Cycle`.
   - otherwise build `FrameKey { key, parent }`.
   - active entry for that exact frame key -> return a future that polls `frames[frame_id].result`.
   - no entry -> allocate a new `ProofFrame { result: None }`, insert `FrameKey -> FrameId`, and return a future that polls that frame.
3. The caller that created the new frame is responsible for driving the actual atomic proof and then calling `finish(frame, result)`.
4. Waiters and the owner all observe completion through the returned `ProofFuture`.
5. The MVP does not remove entries from the whiteboard. The whole whiteboard is scoped to one `GoalQuery::prove` invocation, so completed frames can remain until the invocation finishes.

This means an exact duplicate under the same parent awaits the first proof, while direct or indirect recursion through parent frames becomes `No`.
