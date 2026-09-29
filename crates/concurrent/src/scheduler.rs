use crate::fiber::Fiber;
use crate::pool::Pool;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, ThreadId};

struct Shared {
    ready: Mutex<VecDeque<Arc<TaskSignal>>>,
    wakeup: Condvar,
}

struct TaskSignal {
    id: u64,
    shared: Weak<Shared>,
    queued: AtomicBool,
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
        if self.completed.load(Ordering::Acquire) || self.queued.load(Ordering::Relaxed) {
            return false;
        }

        self.queued.store(true, Ordering::Release);
        ready.push_back(Arc::clone(self));
        drop(ready);
        shared.wakeup.notify_one();
        true
    }
}

struct FutureWake {
    signal: Arc<TaskSignal>,
}

impl Wake for FutureWake {
    fn wake(self: Arc<Self>) {
        self.signal.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.signal.schedule();
    }
}

struct Task {
    fiber: Fiber,
    signal: Arc<TaskSignal>,
}

impl Drop for Task {
    fn drop(&mut self) {
        self.signal.completed.store(true, Ordering::Release);
    }
}

/// A handle for waking one scheduler task.
///
/// Handles may be sent between threads. Unblocking only queues the task; its
/// fiber always resumes on the thread that owns the scheduler.
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

    /// Return whether the scheduler has observed this task's completion.
    pub fn is_finished(&self) -> bool {
        self.signal.completed.load(Ordering::Acquire)
    }
}

/// A thread-safe handle for sending wakeups to a scheduler.
#[derive(Clone)]
pub struct SchedulerHandle {
    shared: Arc<Shared>,
}

impl SchedulerHandle {
    /// Queue a task that belongs to this scheduler.
    pub fn unblock(&self, task: &TaskHandle) -> bool {
        task.signal
            .shared
            .upgrade()
            .filter(|shared| Arc::ptr_eq(shared, &self.shared))
            .is_some_and(|_| task.signal.schedule())
    }
}

/// A single-threaded executor that runs futures inside stackful fibers.
///
/// Future wakeups are thread-safe, but fiber stacks never migrate between OS
/// threads. The scheduler is not Send.
pub struct Scheduler {
    shared: Arc<Shared>,
    tasks: HashMap<u64, Task>,
    next_id: u64,
    pool: Pool,
    owner: ThreadId,
    _thread_affine: PhantomData<Rc<()>>,
}

impl Scheduler {
    /// Create a scheduler with the given usable stack size for each fiber.
    pub fn new(stack_size: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                ready: Mutex::new(VecDeque::new()),
                wakeup: Condvar::new(),
            }),
            tasks: HashMap::new(),
            next_id: 0,
            pool: Pool::new(stack_size),
            owner: thread::current().id(),
            _thread_affine: PhantomData,
        }
    }

    /// Return a thread-safe handle for external wakeups.
    pub fn handle(&self) -> SchedulerHandle {
        SchedulerHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Spawn a future whose output is discarded.
    pub fn spawn<F>(&mut self, future: F) -> std::io::Result<TaskHandle>
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_task(move |signal| poll_future(future, signal))
    }

    /// Spawn stackful synchronous code on a fiber.
    ///
    /// The closure can call Scheduler::block_current and later be made runnable
    /// with its TaskHandle::unblock method.
    pub fn spawn_fiber(&mut self, entry: impl FnOnce() + 'static) -> std::io::Result<TaskHandle> {
        self.spawn_task(move |_| entry())
    }

    fn spawn_task(
        &mut self,
        entry: impl FnOnce(Arc<TaskSignal>) + 'static,
    ) -> std::io::Result<TaskHandle> {
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
            completed: AtomicBool::new(false),
        });
        let task_signal = Arc::clone(&signal);
        let stack = self.pool.acquire()?;
        let fiber = Fiber::with_stack(stack, move || entry(task_signal))?;

        self.tasks.insert(
            id,
            Task {
                fiber,
                signal: Arc::clone(&signal),
            },
        );
        signal.schedule();

        Ok(TaskHandle { signal })
    }

    /// Yield the currently running scheduler task to the scheduler.
    ///
    /// Use TaskHandle::unblock or a future waker to make it runnable again.
    pub fn block_current() {
        Fiber::yield_now();
    }

    /// Queue a task for execution if it is not already queued or complete.
    pub fn unblock(&self, task: &TaskHandle) -> bool {
        self.handle().unblock(task)
    }

    /// Run ready tasks until none are immediately runnable.
    ///
    /// Returns the number of fiber resumes performed. It can be called again
    /// after an external wakeup.
    pub fn run_until_stalled(&mut self) -> usize {
        self.assert_owner();
        let mut resumed = 0;

        while let Some(signal) = self.pop_ready() {
            let id = signal.id;
            let Some(task) = self.tasks.get_mut(&id) else {
                continue;
            };

            resumed += 1;
            let result = catch_unwind(AssertUnwindSafe(|| task.fiber.resume()));
            let finished = task.fiber.is_finished();

            if finished {
                self.finish_task(id);
            }

            if let Err(payload) = result {
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

    fn finish_task(&mut self, id: u64) {
        if let Some(mut task) = self.tasks.remove(&id) {
            task.signal.completed.store(true, Ordering::Release);
            if let Some(stack) = task.fiber.take_stack() {
                self.pool.recycle(stack);
            }
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

fn poll_future<F>(future: F, signal: Arc<TaskSignal>)
where
    F: Future<Output = ()> + 'static,
{
    let mut future = Box::pin(future);
    let waker = Waker::from(Arc::new(FutureWake { signal }));
    let mut context = Context::from_waker(&waker);

    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(()) => return,
            Poll::Pending => Fiber::yield_now(),
        }
    }
}
