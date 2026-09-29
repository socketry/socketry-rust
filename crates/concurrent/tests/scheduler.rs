use socketry_concurrent::{Scheduler, SchedulerHandle, TaskHandle};
use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc;
use std::task::{Context, Poll};

const STACK_SIZE: usize = 64 * 1024;

#[test]
fn scheduler_repolls_a_future_that_wakes_itself() {
    struct WakeOnce {
        polled: bool,
        completed: Rc<Cell<bool>>,
    }

    impl Future for WakeOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            if self.polled {
                self.completed.set(true);
                Poll::Ready(())
            } else {
                self.polled = true;
                context.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    let completed = Rc::new(Cell::new(false));
    let future_completed = Rc::clone(&completed);
    let mut scheduler = Scheduler::new(STACK_SIZE);

    let task = scheduler
        .spawn(WakeOnce {
            polled: false,
            completed: future_completed,
        })
        .expect("task stack should be allocated");

    assert_eq!(scheduler.run_until_stalled(), 2);
    assert!(completed.get());
    assert!(task.is_finished());
    assert_eq!(scheduler.task_count(), 0);
    assert_eq!(scheduler.cached_stacks(), 1);
}

#[test]
fn scheduler_can_unblock_a_fiber_from_another_thread() {
    let started = Rc::new(Cell::new(false));
    let completed = Rc::new(Cell::new(false));
    let fiber_started = Rc::clone(&started);
    let fiber_completed = Rc::clone(&completed);
    let (started_sender, started_receiver) = mpsc::channel();
    let mut scheduler = Scheduler::new(STACK_SIZE);

    let task = scheduler
        .spawn_fiber(move || {
            fiber_started.set(true);
            started_sender
                .send(())
                .expect("wake thread should be waiting");
            Scheduler::block_current();
            fiber_completed.set(true);
        })
        .expect("task stack should be allocated");

    let scheduler_handle = scheduler.handle();
    let wake_task = task.clone();
    let wake_thread = std::thread::spawn(move || {
        started_receiver
            .recv()
            .expect("fiber should announce that it is blocked");
        assert!(scheduler_handle.unblock(&wake_task));
    });

    scheduler.run();
    wake_thread.join().expect("wake thread should complete");

    assert!(started.get());
    assert!(completed.get());
    assert!(task.is_finished());
    assert_eq!(scheduler.task_count(), 0);
    assert_eq!(scheduler.cached_stacks(), 1);
}

#[test]
fn scheduler_rejects_a_task_from_another_scheduler() {
    let mut first_scheduler = Scheduler::new(STACK_SIZE);
    let mut second_scheduler = Scheduler::new(STACK_SIZE);
    let task = first_scheduler
        .spawn(async {})
        .expect("task stack should be allocated");
    let second_scheduler_handle: SchedulerHandle = second_scheduler.handle();

    assert!(!second_scheduler_handle.unblock(&task));
    assert_eq!(first_scheduler.run_until_stalled(), 1);
    assert_eq!(second_scheduler.run_until_stalled(), 0);
}

#[test]
fn task_handles_can_be_cloned_without_changing_task_identity() {
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task: TaskHandle = scheduler
        .spawn(async {})
        .expect("task stack should be allocated");
    let cloned_task = task.clone();

    assert_eq!(task.id(), cloned_task.id());
    assert_eq!(scheduler.run_until_stalled(), 1);
    assert!(cloned_task.is_finished());
}
