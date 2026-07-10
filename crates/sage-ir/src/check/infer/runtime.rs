use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ty::InferVarIndex;

type BoxFuture = Pin<Box<dyn Future<Output = ()>>>;

/// Identifier for a runtime task.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(u32);

pub(crate) struct ReadyQueue<T> {
    queue: VecDeque<T>,
    queued: FxHashSet<T>,
}

impl<T> Default for ReadyQueue<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            queued: FxHashSet::default(),
        }
    }
}

impl<T: Copy + Eq + Hash> ReadyQueue<T> {
    pub(crate) fn enqueue(&mut self, value: T) {
        if self.queued.insert(value) {
            self.queue.push_back(value);
        }
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        let value = self.queue.pop_front()?;
        self.queued.remove(&value);
        Some(value)
    }

    pub(crate) fn remove(&mut self, value: T) {
        if self.queued.remove(&value) {
            self.queue.retain(|queued| *queued != value);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

struct TaskWake {
    task: TaskId,
    ready: Arc<Mutex<ReadyQueue<TaskId>>>,
    coordinator: Option<Waker>,
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.ready.lock().unwrap().enqueue(self.task);
        if let Some(coordinator) = &self.coordinator {
            coordinator.wake_by_ref();
        }
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.lock().unwrap().enqueue(self.task);
        if let Some(coordinator) = &self.coordinator {
            coordinator.wake_by_ref();
        }
    }
}

fn task_waker(task: TaskId, ready: &Arc<Mutex<ReadyQueue<TaskId>>>) -> Waker {
    Waker::from(Arc::new(TaskWake {
        task,
        ready: ready.clone(),
        coordinator: None,
    }))
}

fn scoped_task_waker(
    task: TaskId,
    ready: &Arc<Mutex<ReadyQueue<TaskId>>>,
    coordinator: &Waker,
) -> Waker {
    Waker::from(Arc::new(TaskWake {
        task,
        ready: ready.clone(),
        coordinator: Some(coordinator.clone()),
    }))
}

/// Single-threaded cooperative scheduler for body inference tasks.
pub struct Runtime {
    next_id: u32,
    ready: Arc<Mutex<ReadyQueue<TaskId>>>,
    tasks: FxHashMap<TaskId, BoxFuture>,
    waiting: FxHashMap<InferVarIndex, Vec<(TaskId, Waker)>>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            ready: Arc::new(Mutex::new(ReadyQueue::default())),
            tasks: FxHashMap::default(),
            waiting: FxHashMap::default(),
        }
    }

    pub fn alloc_task_id(&mut self) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn spawn(&mut self, future: impl Future<Output = ()> + 'static) {
        let id = self.alloc_task_id();
        assert!(self.tasks.insert(id, Box::pin(future)).is_none());
        self.ready.lock().unwrap().enqueue(id);
    }

    pub fn wait_on(&mut self, var: InferVarIndex, task_id: TaskId, waker: &Waker) {
        let waiters = self.waiting.entry(var).or_default();
        if !waiters
            .iter()
            .any(|(existing, registered)| *existing == task_id && registered.will_wake(waker))
        {
            waiters.push((task_id, waker.clone()));
        }
    }

    pub fn wake_variable(&mut self, var: InferVarIndex) {
        if let Some(waiters) = self.waiting.remove(&var) {
            for (_, waker) in waiters {
                waker.wake();
            }
        }
    }

    pub fn wake_all(&mut self) {
        for (_, waiters) in self.waiting.drain() {
            for (_, waker) in waiters {
                waker.wake();
            }
        }
        let mut ready = self.ready.lock().unwrap();
        for task in self.tasks.keys().copied() {
            ready.enqueue(task);
        }
    }

    /// Poll ready tasks until the real-waker queue is empty.
    pub fn drain(&mut self) {
        while let Some(id) = { self.ready.lock().unwrap().pop() } {
            let Some(future) = self.tasks.get_mut(&id) else {
                continue;
            };
            let waker = task_waker(id, &self.ready);
            let mut context = Context::from_waker(&waker);
            CURRENT_TASK.with(|task| *task.borrow_mut() = Some(id));
            let poll = future.as_mut().poll(&mut context);
            CURRENT_TASK.with(|task| *task.borrow_mut() = None);
            if poll.is_ready() {
                self.tasks.remove(&id);
                for waiters in self.waiting.values_mut() {
                    waiters.retain(|(task, _)| *task != id);
                }
            }
        }
    }

    pub fn block_on<F: Future>(&mut self, future: F) -> F::Output {
        let mut future = Box::pin(future);
        let main = self.alloc_task_id();
        let main_ready = Arc::new(Mutex::new(ReadyQueue::default()));
        main_ready.lock().unwrap().enqueue(main);
        loop {
            let should_poll = main_ready.lock().unwrap().pop().is_some();
            if should_poll {
                let waker = task_waker(main, &main_ready);
                let mut context = Context::from_waker(&waker);
                CURRENT_TASK.with(|task| *task.borrow_mut() = Some(main));
                let poll = future.as_mut().poll(&mut context);
                CURRENT_TASK.with(|task| *task.borrow_mut() = None);
                if let Poll::Ready(output) = poll {
                    self.drain();
                    return output;
                }
            }
            self.drain();
            if main_ready.lock().unwrap().is_empty() && self.ready.lock().unwrap().is_empty() {
                panic!("deadlock: main task pending with no wake source");
            }
        }
    }

    pub fn is_quiescent(&self) -> bool {
        self.tasks.is_empty() && self.ready.lock().unwrap().is_empty()
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

/// A joinable set of futures which may borrow from their creating scope.
///
/// Dropping or calling `cancel_all` drops every pending future before this
/// value can leave the borrowing scope. Branch cleanup is therefore expressed
/// with ordinary future/guard `Drop` implementations.
pub struct ScopedTasks<'scope, T> {
    ready: Arc<Mutex<ReadyQueue<TaskId>>>,
    tasks: Vec<Option<Pin<Box<dyn Future<Output = T> + 'scope>>>>,
    live: usize,
}

impl<'scope, T> ScopedTasks<'scope, T> {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(Mutex::new(ReadyQueue::default())),
            tasks: Vec::new(),
            live: 0,
        }
    }

    pub fn spawn(&mut self, future: impl Future<Output = T> + 'scope) -> TaskId {
        let id = TaskId(self.tasks.len() as u32);
        self.tasks.push(Some(Box::pin(future)));
        self.live += 1;
        self.ready.lock().unwrap().enqueue(id);
        id
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Await the next completed task in scheduler completion order.
    pub async fn next_completed(&mut self) -> Option<(TaskId, T)> {
        poll_fn(|context| self.poll_next(context)).await
    }

    pub fn poll_next(&mut self, context: &mut Context<'_>) -> Poll<Option<(TaskId, T)>> {
        loop {
            let next = { self.ready.lock().unwrap().pop() };
            let Some(id) = next else {
                if self.live == 0 {
                    return Poll::Ready(None);
                }
                return Poll::Pending;
            };
            let Some(future) = self.tasks[id.0 as usize].as_mut() else {
                continue;
            };
            let waker = scoped_task_waker(id, &self.ready, context.waker());
            let mut child_context = Context::from_waker(&waker);
            if let Poll::Ready(output) = future.as_mut().poll(&mut child_context) {
                self.tasks[id.0 as usize] = None;
                self.live -= 1;
                return Poll::Ready(Some((id, output)));
            }
        }
    }

    pub fn cancel_all(&mut self) {
        for task in &mut self.tasks {
            *task = None;
        }
        self.live = 0;
        *self.ready.lock().unwrap() = ReadyQueue::default();
    }
}

impl<T> Default for ScopedTasks<'_, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for ScopedTasks<'_, T> {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

thread_local! {
    pub static CURRENT_TASK: RefCell<Option<TaskId>> = const { RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct WakeOnce {
        polls: usize,
    }

    impl Future for WakeOnce {
        type Output = usize;

        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            self.polls += 1;
            if self.polls == 1 {
                context.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(self.polls)
            }
        }
    }

    #[test]
    fn scoped_tasks_use_real_idempotent_wakers() {
        let mut runtime = Runtime::new();
        let result = runtime.block_on(async {
            let borrowed = 40;
            let mut tasks = ScopedTasks::new();
            tasks.spawn(async { borrowed + WakeOnce { polls: 0 }.await });
            tasks.next_completed().await.unwrap().1
        });
        assert_eq!(result, 42);
    }

    struct DropCount(Arc<AtomicUsize>);

    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn cancellation_drops_every_pending_borrowed_task() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut tasks = ScopedTasks::<()>::new();
        for _ in 0..3 {
            let guard = DropCount(drops.clone());
            tasks.spawn(async move {
                let _guard = guard;
                std::future::pending().await
            });
        }
        tasks.cancel_all();
        assert!(tasks.is_empty());
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }
}
