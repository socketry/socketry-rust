use crate::context::Context as CoroutineContext;
use crate::coroutine::{Coroutine, CoroutineHandle};
use crate::pool::Pool;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, ThreadId};

struct Shared {
    ready: Mutex<VecDeque<Arc<TaskSignal>>>,
    completed: Mutex<VecDeque<u64>>,
    wakeup: Condvar,
}

struct TaskSignal {
    id: u64,
    shared: Weak<Shared>,
    queued: AtomicBool,
    notified: AtomicBool,
    completed: AtomicBool,
}

impl TaskSignal {
    fn schedule(self: &Arc<Self>) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };

        let mut ready = shared
            .ready
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.completed.load(Ordering::Acquire) {
            return false;
        }

        self.notified.store(true, Ordering::Release);
        if self.queued.load(Ordering::Relaxed) {
            return false;
        }

        self.queued.store(true, Ordering::Release);
        ready.push_back(Arc::clone(self));
        drop(ready);
        shared.wakeup.notify_one();
        true
    }

    fn mark_completed(&self) {
        if self.completed.swap(true, Ordering::AcqRel) {
            return;
        }

        if let Some(shared) = self.shared.upgrade() {
            shared
                .completed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push_back(self.id);
        }
    }
}

impl Wake for TaskSignal {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

/// A thread-safe handle for waking one scheduler task.
///
/// Unblocking only queues the task; its stack always resumes on the thread
/// that owns the scheduler.
#[derive(Clone)]
pub struct TaskHandle {
    signal: Arc<TaskSignal>,
}

impl TaskHandle {
    /// Task identifier, unique within its scheduler.
    pub fn id(&self) -> u64 {
        self.signal.id
    }

    /// Queue this task for execution if it is not already queued or complete.
    pub fn unblock(&self) -> bool {
        self.signal.schedule()
    }

    /// Return whether this task has completed.
    pub fn is_finished(&self) -> bool {
        self.signal.completed.load(Ordering::Acquire)
    }

    pub(crate) fn mark_completed(&self) {
        self.signal.mark_completed();
    }
}

/// A thread-affine reference to a currently running scheduler task.
///
/// This reference can be used for stackful task operations such as transfer.
/// It does not own the task or its stack. Use `TaskHandle` when a task needs to
/// be woken from another thread.
#[derive(Clone)]
pub struct Task {
    coroutine: CoroutineHandle,
    handle: TaskHandle,
    scheduler: SchedulerHandle,
}

impl Task {
    /// Return a thread-affine reference to the currently running scheduler
    /// task, if there is one.
    pub fn current() -> Option<Self> {
        let (coroutine, scheduler, handle) = crate::coroutine::current_task_context()?;
        Some(Self {
            coroutine,
            handle,
            scheduler,
        })
    }

    /// Task identifier, unique within its scheduler.
    pub fn id(&self) -> u64 {
        self.handle.id()
    }

    /// Return the thread-safe handle for waking this task.
    pub fn handle(&self) -> TaskHandle {
        self.handle.clone()
    }

    /// Return a thread-safe handle for this task's scheduler.
    pub fn scheduler(&self) -> SchedulerHandle {
        self.scheduler.clone()
    }

    /// Park this task and return control to its scheduler.
    ///
    /// The task can be made runnable through its `TaskHandle`.
    pub fn block(&self) {
        self.assert_current();
        Coroutine::park();
    }

    /// Transfer execution from this task to another task on the same thread.
    ///
    /// This suspends the current task at the call site. It resumes only when
    /// another task transfers back to it or its scheduler runs it again.
    /// Returning from the target task returns control to the scheduler.
    pub fn transfer_to(&self, target: &Task) {
        self.assert_current();
        assert!(
            Arc::ptr_eq(&self.scheduler.shared, &target.scheduler.shared),
            "cannot transfer between different schedulers"
        );
        Coroutine::transfer(&target.coroutine);
    }

    fn assert_current(&self) {
        assert!(
            self.coroutine.is_current(),
            "task operation called outside the referenced task"
        );
    }
}

/// A thread-safe handle for sending wakeups to a scheduler.
#[derive(Clone)]
pub struct SchedulerHandle {
    shared: Arc<Shared>,
}

impl SchedulerHandle {
    /// Wait for a future from an ordinary function within one of this scheduler's tasks.
    ///
    /// This suspends the current task's stack when the future is pending. It
    /// accepts borrowed and non-Send futures, and returns their output directly.
    ///
    /// # Panics
    ///
    /// Panics outside a task belonging to this scheduler.
    pub fn wait<F: Future>(&self, future: F) -> F::Output {
        let task =
            crate::coroutine::current_task_handle().expect("cannot wait outside a scheduler task");
        assert!(
            std::ptr::eq(task.signal.shared.as_ptr(), Arc::as_ptr(&self.shared)),
            "cannot wait using a different scheduler"
        );
        wait_with_signal(future, task.signal)
    }

    /// Queue a task that belongs to this scheduler.
    pub fn unblock(&self, task: &TaskHandle) -> bool {
        task.signal
            .shared
            .upgrade()
            .filter(|shared| Arc::ptr_eq(shared, &self.shared))
            .is_some_and(|_| task.signal.schedule())
    }
}

/// A single-threaded future executor with stackful support for nested waits.
///
/// Future wakeups are thread-safe, but task stacks never migrate between OS
/// threads. The scheduler is not Send.
pub struct Scheduler {
    shared: Arc<Shared>,
    tasks: HashMap<u64, Coroutine>,
    context: CoroutineContext,
    next_id: u64,
    pool: Pool,
    owner: ThreadId,
    _thread_affine: PhantomData<Rc<()>>,
}

impl Scheduler {
    /// Create a scheduler with the given usable stack size for each task.
    pub fn new(stack_size: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                ready: Mutex::new(VecDeque::new()),
                completed: Mutex::new(VecDeque::new()),
                wakeup: Condvar::new(),
            }),
            tasks: HashMap::new(),
            context: CoroutineContext::empty(),
            next_id: 0,
            pool: Pool::new(stack_size),
            owner: thread::current().id(),
            _thread_affine: PhantomData,
        }
    }

    /// Return a handle to the scheduler associated with the currently running
    /// task, if it was started by a scheduler.
    ///
    /// This performs a thread-local active-task lookup and clones the
    /// scheduler's shared handle. It returns `None` outside a scheduler task.
    pub fn current() -> Option<SchedulerHandle> {
        crate::coroutine::current_scheduler_handle()
    }

    /// Return a thread-safe handle for external wakeups.
    pub fn handle(&self) -> SchedulerHandle {
        SchedulerHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Spawn a future on its own coroutine stack.
    ///
    /// The future may use `.await`, call ordinary functions that use [`wait`],
    /// or both. A nested wait resumes the existing poll invocation before the
    /// outer future can be polled again. Futures need not implement Send.
    pub fn spawn<F>(&mut self, future: F) -> std::io::Result<TaskHandle>
    where
        F: Future<Output = ()> + 'static,
    {
        self.assert_owner();

        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("scheduler task identifier space exhausted");
        let signal = Arc::new(TaskSignal {
            id,
            shared: Arc::downgrade(&self.shared),
            queued: AtomicBool::new(false),
            notified: AtomicBool::new(false),
            completed: AtomicBool::new(false),
        });
        let task_signal = Arc::clone(&signal);
        let stack = self.pool.acquire()?;
        let mut coroutine =
            Coroutine::with_stack(stack, move || wait_with_signal(future, task_signal))?;
        coroutine.set_scheduler(
            self.handle(),
            TaskHandle {
                signal: Arc::clone(&signal),
            },
        );

        self.tasks.insert(id, coroutine);
        signal.schedule();

        Ok(TaskHandle { signal })
    }

    /// Park the currently running scheduler task and return to the scheduler.
    ///
    /// Use `TaskHandle::unblock` or a future waker to make it runnable again.
    pub fn block_current() {
        Coroutine::park();
    }

    /// Queue a task for execution if it is not already queued or complete.
    pub fn unblock(&self, task: &TaskHandle) -> bool {
        self.handle().unblock(task)
    }

    /// Run ready tasks until none are immediately runnable.
    ///
    /// Returns the number of task dispatches performed. It can be called again
    /// after an external wakeup.
    pub fn run_until_stalled(&mut self) -> usize {
        self.assert_owner();
        let mut resumed = 0;

        while let Some(signal) = self.pop_ready() {
            let id = signal.id;
            let result = match self.tasks.get_mut(&id) {
                Some(task) => {
                    resumed += 1;
                    catch_unwind(AssertUnwindSafe(|| task.dispatch(&mut self.context)))
                }
                None => continue,
            };

            // Transfer can finish a different task while the scheduler is
            // running this one. Finished tasks report themselves here, so
            // cleanup work is proportional to completions rather than the
            // number of tasks owned by the scheduler.
            let mut completion_panic = None;
            while let Some(identifier) = self.pop_completed() {
                if completion_panic.is_none() {
                    completion_panic = self.finish_task(identifier);
                } else {
                    self.finish_task(identifier);
                }
            }

            if let Err(payload) = result {
                resume_unwind(payload);
            }
            if let Some(payload) = completion_panic {
                resume_unwind(payload);
            }
        }

        resumed
    }

    /// Run until every task completes, waiting for external wakeups as needed.
    pub fn run(&mut self) {
        self.assert_owner();

        while !self.tasks.is_empty() {
            self.run_until_stalled();
            if self.tasks.is_empty() {
                break;
            }

            let mut ready = self
                .shared
                .ready
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while ready.is_empty() && !self.tasks.is_empty() {
                ready = self
                    .shared
                    .wakeup
                    .wait(ready)
                    .unwrap_or_else(|error| error.into_inner());
            }
        }
    }

    /// Number of unfinished tasks owned by this scheduler.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Number of stacks cached for reuse.
    pub fn cached_stacks(&self) -> usize {
        self.pool.cached()
    }

    fn pop_ready(&self) -> Option<Arc<TaskSignal>> {
        let mut ready = self
            .shared
            .ready
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let signal = ready.pop_front()?;
        signal.queued.store(false, Ordering::Release);
        Some(signal)
    }

    fn pop_completed(&self) -> Option<u64> {
        self.shared
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
    }

    fn finish_task(&mut self, id: u64) -> Option<Box<dyn std::any::Any + Send + 'static>> {
        if let Some(mut coroutine) = self.tasks.remove(&id) {
            let panic = coroutine.take_panic();
            if let Some(stack) = coroutine.take_stack() {
                self.pool.recycle(stack);
            }
            panic
        } else {
            None
        }
    }

    fn assert_owner(&self) {
        assert_eq!(
            thread::current().id(),
            self.owner,
            "Scheduler must run on its owning thread"
        );
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        for task in self.tasks.values_mut() {
            let _ = catch_unwind(AssertUnwindSafe(|| task.cancel(&mut self.context)));
        }
    }
}

/// Wait for a future without making the calling function async.
///
/// A pending future suspends the current task's coroutine stack so other
/// tasks can run. When woken, the same stack resumes and polls the future
/// again. Calls may be nested, including inside another future's `poll`.
///
/// The future is pinned on the task stack; this function does not allocate a
/// separate future or waker. Borrowed, non-Send, and non-Unpin futures are
/// supported. Futures that depend on another runtime still need that runtime's
/// services to be running.
///
/// # Panics
///
/// Panics outside a scheduler task. Future panics unwind through the caller.
///
/// # Example
///
/// ```
/// use socketry_concurrent::{Scheduler, wait};
/// use std::future::poll_fn;
/// use std::task::Poll;
///
/// fn answer() -> usize {
///     let mut first_poll = true;
///     wait(poll_fn(|context| {
///         if first_poll {
///             first_poll = false;
///             context.waker().wake_by_ref();
///             Poll::Pending
///         } else {
///             Poll::Ready(42)
///         }
///     }))
/// }
///
/// let mut scheduler = Scheduler::new(256 * 1024);
/// scheduler.spawn(async { assert_eq!(answer(), 42); })?;
/// scheduler.run();
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn wait<F: Future>(future: F) -> F::Output {
    let task =
        crate::coroutine::current_task_handle().expect("cannot wait outside a scheduler task");
    wait_with_signal(future, task.signal)
}

// A nested wait can consume a ready-queue entry caused by an outer future's
// waker. Remember every notification consumed by this wait and restore it to
// the enclosing poll when returning or unwinding. Only the owning thread
// clears notifications; external threads only set them through TaskSignal.
struct WakeScope {
    signal: Arc<TaskSignal>,
    observed_wakeup: bool,
}

impl Drop for WakeScope {
    fn drop(&mut self) {
        if self.observed_wakeup {
            self.signal.notified.store(true, Ordering::Release);
        }
    }
}

fn wait_with_signal<F: Future>(future: F, signal: Arc<TaskSignal>) -> F::Output {
    let mut scope = WakeScope {
        signal,
        observed_wakeup: false,
    };
    let mut future = pin!(future);
    let waker = Waker::from(Arc::clone(&scope.signal));
    let mut context = Context::from_waker(&waker);

    loop {
        scope.observed_wakeup |= scope.signal.notified.swap(false, Ordering::AcqRel);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                // A nested wait may have consumed the ready entry. Requeue
                // when this poll was notified, even if that notification was
                // restored by a nested scope. schedule() coalesces duplicates.
                if scope.signal.notified.load(Ordering::Acquire) {
                    scope.signal.schedule();
                }
                Coroutine::park();
            }
        }
    }
}
