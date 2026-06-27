use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use rustc_hash::FxHashMap;

use crate::ty::InferVarIndex;

type BoxFuture = Pin<Box<dyn Future<Output = ()>>>;

/// Identifier for a spawned task.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(u32);

/// A spawned task in the runtime.
struct Task {
    id: TaskId,
    future: BoxFuture,
}

/// Single-threaded cooperative scheduler for type inference.
///
/// Tasks await inference variable updates. When a variable's bound tightens,
/// all tasks waiting on it are re-queued.
pub struct Runtime {
    next_id: u32,
    ready: VecDeque<Task>,
    /// Per-inference-variable waker lists: tasks waiting for a bound update.
    waiting: FxHashMap<InferVarIndex, Vec<TaskId>>,
    /// Map from task id to the actual task (when suspended).
    suspended: FxHashMap<TaskId, Task>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            ready: VecDeque::new(),
            waiting: FxHashMap::default(),
            suspended: FxHashMap::default(),
        }
    }

    /// Allocate a task ID without spawning (used for the main task in block_on).
    pub fn alloc_task_id(&mut self) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Spawn a new task.
    pub fn spawn(&mut self, future: impl Future<Output = ()> + 'static) {
        let id = self.alloc_task_id();
        self.ready.push_back(Task {
            id,
            future: Box::pin(future),
        });
    }

    /// Register a task as waiting on an inference variable.
    pub fn wait_on(&mut self, var: InferVarIndex, task_id: TaskId) {
        self.waiting.entry(var).or_default().push(task_id);
    }

    /// Wake all tasks waiting on a variable (called when a bound tightens).
    pub fn wake_variable(&mut self, var: InferVarIndex) {
        if let Some(waiters) = self.waiting.remove(&var) {
            for task_id in waiters {
                if let Some(task) = self.suspended.remove(&task_id) {
                    self.ready.push_back(task);
                }
            }
        }
    }

    /// Wake all suspended tasks (used during finalization).
    pub fn wake_all(&mut self) {
        for (_var, waiters) in self.waiting.drain() {
            for task_id in waiters {
                if let Some(task) = self.suspended.remove(&task_id) {
                    self.ready.push_back(task);
                }
            }
        }
        for (_id, task) in self.suspended.drain() {
            self.ready.push_back(task);
        }
    }

    /// Drain the ready queue: poll each task once. Tasks that return
    /// `Poll::Pending` are moved to `suspended`. Returns when the ready
    /// queue is empty.
    pub fn drain(&mut self) {
        while let Some(mut task) = self.ready.pop_front() {
            let waker = Waker::noop();
            let mut cx = Context::from_waker(&waker);
            match task.future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {}
                Poll::Pending => {
                    self.suspended.insert(task.id, task);
                }
            }
        }
    }

    /// Run a future to completion as the main task. Drains the scheduler
    /// after the main task completes to let spawned tasks finish.
    pub fn block_on<F: Future>(&mut self, future: F) -> F::Output {
        let mut future = Box::pin(future);
        loop {
            let waker = Waker::noop();
            let mut cx = Context::from_waker(&waker);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => {
                    self.drain();
                    return result;
                }
                Poll::Pending => {
                    self.drain();
                    if self.ready.is_empty() && self.suspended.is_empty() {
                        panic!("deadlock: main task pending with no runnable or suspended tasks");
                    }
                }
            }
        }
    }

    /// True if there are no more suspended tasks.
    pub fn is_quiescent(&self) -> bool {
        self.ready.is_empty() && self.suspended.is_empty()
    }
}

thread_local! {
    pub static CURRENT_TASK: RefCell<Option<TaskId>> = const { RefCell::new(None) };
}
