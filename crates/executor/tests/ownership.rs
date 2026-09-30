mod support;

use socketry_executor::{Scheduler, Spawn, SpawnError, TaskError, yield_now};
use std::future::{Future, pending, poll_fn};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::task::Poll;
use support::{CountDrop, receive};

#[test]
fn barriers_and_schedulers_share_the_spawn_contract() {
    fn submit(owner: &impl Spawn) -> impl Future<Output = Result<usize, TaskError>> + Send {
        owner
            .spawn(async {
                yield_now().await;
                42
            })
            .unwrap()
    }
    let scheduler = Scheduler::with_workers(2).unwrap();
    let barrier = scheduler.barrier();
    assert_eq!(scheduler.block_on(submit(&scheduler)).unwrap(), 42);
    assert_eq!(scheduler.block_on(submit(&barrier)).unwrap(), 42);
    barrier.close();
    scheduler.block_on(barrier.wait());
    assert!(barrier.is_empty());
    assert!(matches!(
        barrier.spawn(async {}),
        Err(SpawnError::OwnerClosed)
    ));
}

#[test]
fn barrier_waits_after_join_handles_are_dropped() {
    let scheduler = Scheduler::with_workers(4).unwrap();
    let barrier = scheduler.barrier();
    let completed = Arc::new(AtomicUsize::new(0));
    for _ in 0..500 {
        let completed = Arc::clone(&completed);
        drop(
            barrier
                .spawn(async move {
                    yield_now().await;
                    completed.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap(),
        );
    }
    barrier.close();
    scheduler.block_on(barrier.wait());
    assert_eq!(completed.load(Ordering::SeqCst), 500);
}

#[test]
fn stop_waits_for_child_destructors() {
    let scheduler = Scheduler::with_workers(2).unwrap();
    let barrier = scheduler.barrier();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..100 {
        let guard = CountDrop(Arc::clone(&dropped));
        handles.push(
            barrier
                .spawn(async move {
                    let _guard = guard;
                    pending::<()>().await;
                })
                .unwrap(),
        );
    }
    scheduler.block_on(barrier.stop());
    assert_eq!(dropped.load(Ordering::SeqCst), 100);
    for handle in handles {
        assert!(matches!(
            scheduler.block_on(handle),
            Err(TaskError::Cancelled)
        ));
    }
}

#[test]
fn dropping_barrier_requests_cancellation() {
    let scheduler = Scheduler::with_workers(2).unwrap();
    let barrier = scheduler.barrier();
    let dropped = Arc::new(AtomicUsize::new(0));
    let guard = CountDrop(Arc::clone(&dropped));
    let handle = barrier
        .spawn(async move {
            let _guard = guard;
            pending::<()>().await;
        })
        .unwrap();
    drop(barrier);
    assert!(matches!(
        scheduler.block_on(handle),
        Err(TaskError::Cancelled)
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_before_first_poll_drops_captured_values() {
    let scheduler = Scheduler::with_workers(1).unwrap();
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    drop(
        scheduler
            .spawn(async move {
                started_sender.send(()).unwrap();
                receive(&release_receiver);
            })
            .unwrap(),
    );
    receive(&started_receiver);
    let polled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicUsize::new(0));
    let task_polled = Arc::clone(&polled);
    let guard = CountDrop(Arc::clone(&dropped));
    let handle = scheduler
        .spawn(async move {
            let _guard = guard;
            task_polled.store(true, Ordering::SeqCst);
        })
        .unwrap();
    assert!(handle.task().cancel());
    release_sender.send(()).unwrap();
    assert!(matches!(
        scheduler.block_on(handle),
        Err(TaskError::Cancelled)
    ));
    assert!(!polled.load(Ordering::SeqCst));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_does_not_destroy_an_in_progress_poll() {
    let scheduler = Scheduler::with_workers(2).unwrap();
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let guard = CountDrop(Arc::clone(&dropped));
    let handle = scheduler
        .spawn(poll_fn(move |_| {
            let _ = &guard;
            started_sender.send(()).unwrap();
            receive(&release_receiver);
            Poll::<()>::Pending
        }))
        .unwrap();
    receive(&started_receiver);
    assert!(handle.task().cancel());
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    release_sender.send(()).unwrap();
    assert!(matches!(
        scheduler.block_on(handle),
        Err(TaskError::Cancelled)
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_method_awaits_cleanup() {
    let scheduler = Scheduler::with_workers(1).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let guard = CountDrop(Arc::clone(&dropped));
    let handle = scheduler
        .spawn(async move {
            let _guard = guard;
            pending::<()>().await;
        })
        .unwrap();
    assert!(matches!(
        scheduler.block_on(handle.cancel()),
        Err(TaskError::Cancelled)
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn child_cannot_wait_for_its_own_barrier() {
    let scheduler = Scheduler::with_workers(1).unwrap();
    let barrier = Arc::new(scheduler.barrier());
    let child_barrier = Arc::clone(&barrier);
    let handle = barrier
        .spawn(async move {
            child_barrier.wait().await;
        })
        .unwrap();
    assert!(matches!(
        scheduler.block_on(handle),
        Err(TaskError::Panicked(_))
    ));
}

#[test]
fn closing_owner_races_safely_with_submission() {
    let scheduler = Scheduler::with_workers(4).unwrap();
    let barrier = Arc::new(scheduler.barrier());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                for _ in 0..100 {
                    if let Ok(handle) = barrier.spawn(async {
                        pending::<()>().await;
                    }) {
                        drop(handle);
                    }
                }
            });
        }
        scheduler.block_on(barrier.stop());
    });
    assert!(barrier.is_empty());
    assert!(matches!(
        barrier.spawn(async {}),
        Err(SpawnError::OwnerClosed)
    ));
}

#[test]
fn cancelling_parent_drops_its_barrier_and_cancels_children() {
    let scheduler = Scheduler::with_workers(2).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let guard = CountDrop(Arc::clone(&dropped));
    let (started_sender, started_receiver) = mpsc::channel();
    let parent = scheduler
        .spawn(async move {
            let children = Scheduler::current().unwrap().barrier();
            drop(
                children
                    .spawn(async move {
                        let _guard = guard;
                        pending::<()>().await;
                    })
                    .unwrap(),
            );
            started_sender.send(()).unwrap();
            pending::<()>().await;
            drop(children);
        })
        .unwrap();
    receive(&started_receiver);
    assert!(matches!(
        scheduler.block_on(parent.cancel()),
        Err(TaskError::Cancelled)
    ));
    // Parent cancellation requests child cancellation. The scheduler's drain
    // supplies the explicit wait for descendant destruction in this test.
    scheduler.run();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn future_destructor_can_submit_work_without_holding_the_registry_lock() {
    struct SubmitOnDrop(mpsc::Sender<u64>);
    impl Drop for SubmitOnDrop {
        fn drop(&mut self) {
            let task = socketry_executor::Task::current().unwrap();
            drop(Scheduler::current().unwrap().spawn(async {}).unwrap());
            self.0.send(task.id()).unwrap();
        }
    }

    let scheduler = Scheduler::with_workers(1).unwrap();
    let (sender, receiver) = mpsc::channel();
    let guard = SubmitOnDrop(sender);
    let handle = scheduler
        .spawn(async move {
            let _guard = guard;
            pending::<()>().await;
        })
        .unwrap();
    let identifier = handle.id();
    assert!(handle.task().cancel());
    assert_eq!(receive(&receiver), identifier);
    assert!(matches!(
        scheduler.block_on(handle),
        Err(TaskError::Cancelled)
    ));
    scheduler.run();
}
