// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use loom::sync::Arc;
use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering, fence};
use loom::thread;

// Model only the worker/enqueuer parking handshake. Crossbeam queue internals
// and async-task internals are not instrumented by this model. A release/acquire
// flag represents publication/observation of a runnable in a concurrent queue.
// Keep the operation order aligned with worker::run and Shared::wake_idle.
#[test]
fn queue_publication_cannot_leave_a_worker_asleep() {
    loom::model(|| {
        let queued = Arc::new(AtomicBool::new(false));
        let sleeping = Arc::new(AtomicBool::new(false));
        let idle_workers = Arc::new(AtomicUsize::new(0));
        let unpark_token = Arc::new(AtomicBool::new(false));

        let worker_queued = Arc::clone(&queued);
        let worker_sleeping = Arc::clone(&sleeping);
        let worker_idle = Arc::clone(&idle_workers);
        let worker = thread::spawn(move || {
            worker_sleeping.store(true, Ordering::SeqCst);
            worker_idle.fetch_add(1, Ordering::SeqCst);
            fence(Ordering::SeqCst);
            // A true result means the worker found nothing on its second search
            // and could enter park. The enqueuer must then leave an unpark token.
            !worker_queued.load(Ordering::Acquire)
        });

        queued.store(true, Ordering::Release);
        fence(Ordering::SeqCst);
        if idle_workers.load(Ordering::SeqCst) != 0 && sleeping.swap(false, Ordering::SeqCst) {
            unpark_token.store(true, Ordering::Release);
        }

        let could_park = worker.join().unwrap();
        assert!(!could_park || unpark_token.load(Ordering::Acquire));
    });
}
