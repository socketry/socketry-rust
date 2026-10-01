// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use crate::scheduler::{self, Shared};
use crate::task::{Runnable, Task, UNASSIGNED_WORKER};
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::thread::{self, Thread};

// Check external work regularly even when the local queue stays busy.
const EXTERNAL_INTERVAL: usize = 61;

pub(crate) struct WorkerState {
    pub(crate) incoming: Injector<Runnable>,
    stealer: Stealer<Runnable>,
    sleeping: AtomicBool,
    thread: OnceLock<Thread>,
}

impl WorkerState {
    pub(crate) fn new(stealer: Stealer<Runnable>) -> Self {
        Self {
            incoming: Injector::new(),
            stealer,
            sleeping: AtomicBool::new(false),
            thread: OnceLock::new(),
        }
    }

    pub(crate) fn wake(&self) -> bool {
        if self.sleeping.swap(false, Ordering::SeqCst) {
            self.thread
                .get()
                .expect("sleeping worker has registered its thread")
                .unpark();
            true
        } else {
            false
        }
    }
}

struct LocalWorker {
    identifier: usize,
    scheduler: Weak<Shared>,
    ready: Worker<Runnable>,
}

thread_local! {
    static CURRENT_WORKER: RefCell<Option<LocalWorker>> = const { RefCell::new(None) };
}

pub(crate) fn is_worker() -> bool {
    CURRENT_WORKER.with(|current| current.borrow().is_some())
}

pub(crate) fn assert_outside_worker() {
    assert!(
        !is_worker(),
        "cannot synchronously wait on a Socketry worker; use .await"
    );
}

pub(crate) fn schedule(shared: &Arc<Shared>, runnable: Runnable) {
    let preferred = runnable.metadata().worker.load(Ordering::Relaxed);
    let remote = CURRENT_WORKER.with(|current| {
        let current = current.borrow();
        if let Some(worker) = current.as_ref()
            && std::ptr::eq(worker.scheduler.as_ptr(), Arc::as_ptr(shared))
            && (preferred == UNASSIGNED_WORKER || preferred == worker.identifier)
        {
            worker.ready.push(runnable);
            return None;
        }
        Some(runnable)
    });
    if let Some(runnable) = remote {
        if let Some(worker) = shared.workers.get(preferred) {
            worker.incoming.push(runnable);
        } else {
            shared.ready.push(runnable);
        }
    }
    // Publication precedes the sleeping flag exchange. A worker publishes its
    // sleeping flag before rechecking queues, so a racing wake is retained.
    shared.wake_idle(preferred);
}

fn search(shared: &Shared, local: &LocalWorker, iteration: usize) -> Steal<Runnable> {
    let incoming = &shared.workers[local.identifier].incoming;
    let mut retry = false;
    if iteration.is_multiple_of(EXTERNAL_INTERVAL) {
        for queue in [&shared.ready, incoming] {
            match queue.steal_batch_and_pop(&local.ready) {
                Steal::Success(runnable) => return Steal::Success(runnable),
                Steal::Retry => retry = true,
                Steal::Empty => {}
            }
        }
    }
    if let Some(runnable) = local.ready.pop() {
        return Steal::Success(runnable);
    }
    for queue in [incoming, &shared.ready] {
        match queue.steal_batch_and_pop(&local.ready) {
            Steal::Success(runnable) => return Steal::Success(runnable),
            Steal::Retry => retry = true,
            Steal::Empty => {}
        }
    }
    // Only steal after exhausting this worker's own work. Rotate the first
    // victim so idle workers do not always contend on worker zero.
    for offset in 0..shared.workers.len() {
        let victim = local
            .identifier
            .wrapping_add(iteration)
            .wrapping_add(offset)
            % shared.workers.len();
        if victim == local.identifier {
            continue;
        }
        let worker = &shared.workers[victim];
        match worker.stealer.steal_batch_and_pop(&local.ready) {
            Steal::Success(runnable) => return Steal::Success(runnable),
            Steal::Retry => retry = true,
            Steal::Empty => {}
        }
        // Remote wakes remain stealable while the previous worker is busy.
        match worker.incoming.steal_batch_and_pop(&local.ready) {
            Steal::Success(runnable) => return Steal::Success(runnable),
            Steal::Retry => retry = true,
            Steal::Empty => {}
        }
    }
    if retry { Steal::Retry } else { Steal::Empty }
}

fn find_work(shared: &Shared, iteration: usize) -> Steal<Runnable> {
    CURRENT_WORKER.with(|current| {
        search(
            shared,
            current
                .borrow()
                .as_ref()
                .expect("worker context is installed"),
            iteration,
        )
    })
}

pub(crate) fn run(shared: Arc<Shared>, identifier: usize, ready: Worker<Runnable>) {
    let _scheduler_scope = scheduler::enter(&shared);
    let worker = &shared.workers[identifier];
    worker
        .thread
        .set(thread::current())
        .expect("worker thread registered once");
    CURRENT_WORKER.with(|current| {
        current.replace(Some(LocalWorker {
            identifier,
            scheduler: Arc::downgrade(&shared),
            ready,
        }))
    });
    let mut iteration = 0usize;
    loop {
        iteration = iteration.wrapping_add(1);
        if shared.finished() {
            break;
        }
        let runnable = match find_work(&shared, iteration) {
            Steal::Success(runnable) => runnable,
            Steal::Retry => {
                thread::yield_now();
                continue;
            }
            Steal::Empty => {
                worker.sleeping.store(true, Ordering::SeqCst);
                shared.idle_workers.fetch_add(1, Ordering::SeqCst);
                // Pair with the enqueuer's fence: either this search observes
                // its queued work, or the enqueuer observes an idle worker.
                std::sync::atomic::fence(Ordering::SeqCst);
                // Recheck after publishing the sleeping flag. An unpark token
                // remains available if work arrives immediately before park().
                match find_work(&shared, iteration) {
                    Steal::Success(runnable) => {
                        worker.sleeping.store(false, Ordering::SeqCst);
                        shared.idle_workers.fetch_sub(1, Ordering::SeqCst);
                        runnable
                    }
                    Steal::Retry => {
                        worker.sleeping.store(false, Ordering::SeqCst);
                        shared.idle_workers.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    Steal::Empty => {
                        if !shared.finished() {
                            thread::park();
                        }
                        worker.sleeping.store(false, Ordering::SeqCst);
                        shared.idle_workers.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                }
            }
        };
        runnable
            .metadata()
            .worker
            .store(identifier, Ordering::Relaxed);
        let _task_scope = Task {
            state: Arc::clone(runnable.metadata()),
        }
        .enter();
        // Fan out work taken in a batch when other workers are parked.
        CURRENT_WORKER.with(|current| {
            if !current
                .borrow()
                .as_ref()
                .expect("worker context is installed")
                .ready
                .is_empty()
            {
                shared.wake_idle(UNASSIGNED_WORKER);
            }
        });
        // Future panics become TaskError in TaskFuture. Also contain a panic
        // propagated by task machinery so that it does not remove a worker.
        let _ = catch_unwind(AssertUnwindSafe(|| runnable.run()));
    }
    CURRENT_WORKER.with(|current| current.take());
}
