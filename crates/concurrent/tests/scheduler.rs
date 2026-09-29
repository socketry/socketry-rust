use socketry_concurrent::{Scheduler, SchedulerHandle, Task, TaskHandle};
use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc;
use std::task::{Context, Poll};

const STACK_SIZE: usize = 64 * 1024;

#[test]
fn scheduler_current_is_none_outside_scheduler_tasks() {
    assert!(Scheduler::current().is_none());
    assert!(Task::current().is_none());
}

#[test]
fn task_current_returns_a_context_for_the_running_task() {
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let blocked_task = scheduler
        .spawn(async { Scheduler::block_current() })
        .expect("task stack should be allocated");

    assert_eq!(scheduler.run_until_stalled(), 1);
    assert!(!blocked_task.is_finished());

    let unblocked_task = Rc::new(Cell::new(false));
    let task_unblocked_task = Rc::clone(&unblocked_task);
    let current_task_id = Rc::new(Cell::new(None));
    let task_current_task_id = Rc::clone(&current_task_id);
    let blocked_task_handle = blocked_task.clone();
    let current_task = scheduler
        .spawn(async move {
            task_current_task_id.set(Task::current().map(|task| task.id()));
            let current = Scheduler::current().expect("scheduler task should have a scheduler");
            task_unblocked_task.set(current.unblock(&blocked_task_handle));
        })
        .expect("task stack should be allocated");

    assert_eq!(scheduler.run_until_stalled(), 2);
    assert!(unblocked_task.get());
    assert_eq!(current_task_id.get(), Some(current_task.id()));
    assert!(blocked_task.is_finished());
    assert!(Scheduler::current().is_none());
}

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
fn scheduler_can_unblock_a_task_from_another_thread() {
    let started = Rc::new(Cell::new(false));
    let completed = Rc::new(Cell::new(false));
    let task_started = Rc::clone(&started);
    let task_completed = Rc::clone(&completed);
    let (started_sender, started_receiver) = mpsc::channel();
    let mut scheduler = Scheduler::new(STACK_SIZE);

    let task = scheduler
        .spawn(async move {
            task_started.set(true);
            started_sender
                .send(())
                .expect("wake thread should be waiting");
            Scheduler::block_current();
            task_completed.set(true);
        })
        .expect("task stack should be allocated");

    let scheduler_handle = scheduler.handle();
    let wake_task = task.clone();
    let wake_thread = std::thread::spawn(move || {
        started_receiver
            .recv()
            .expect("task should announce that it is blocked");
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

#[test]
fn task_completion_returns_to_the_scheduler_without_resuming_its_transfer_source() {
    let target_context = Rc::new(std::cell::RefCell::new(None));
    let source_continued = Rc::new(Cell::new(false));
    let mut scheduler = Scheduler::new(STACK_SIZE);

    let target_slot = Rc::clone(&target_context);
    let target = scheduler
        .spawn(async move {
            let current = Task::current().expect("scheduler task should have a task context");
            *target_slot.borrow_mut() = Some(current.clone());
            current.block();
        })
        .expect("target task stack should be allocated");

    let target_slot = Rc::clone(&target_context);
    let source_continued_signal = Rc::clone(&source_continued);
    let source = scheduler
        .spawn(async move {
            let current = Task::current().expect("scheduler task should have a task context");
            let target = target_slot
                .borrow()
                .as_ref()
                .expect("target task should have started")
                .clone();
            current.transfer_to(&target);
            source_continued_signal.set(true);
        })
        .expect("source task stack should be allocated");

    assert_eq!(scheduler.run_until_stalled(), 2);

    assert!(!source_continued.get());
    assert!(target.is_finished());
    assert!(!source.is_finished());
    assert_eq!(scheduler.task_count(), 1);

    assert!(source.unblock());
    assert_eq!(scheduler.run_until_stalled(), 1);
    assert!(source_continued.get());
    assert_eq!(scheduler.task_count(), 0);
}

#[test]
fn panic_in_transferred_task_reaches_the_scheduler() {
    let target_context = Rc::new(std::cell::RefCell::new(None));
    let mut scheduler = Scheduler::new(STACK_SIZE);

    let target_slot = Rc::clone(&target_context);
    let target = scheduler
        .spawn(async move {
            let current = Task::current().expect("scheduler task should have a task context");
            *target_slot.borrow_mut() = Some(current);
            Scheduler::block_current();
            panic!("transferred task failed");
        })
        .expect("target task stack should be allocated");

    let target_slot = Rc::clone(&target_context);
    let source = scheduler
        .spawn(async move {
            let target = target_slot
                .borrow()
                .as_ref()
                .expect("target task should have started")
                .clone();
            Task::current()
                .expect("scheduler task should have a task context")
                .transfer_to(&target);
        })
        .expect("source task stack should be allocated");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scheduler.run_until_stalled();
    }));

    assert!(result.is_err());
    assert!(target.is_finished());
    assert!(!source.is_finished());
    assert_eq!(scheduler.task_count(), 1);

    assert!(source.unblock());
    assert_eq!(scheduler.run_until_stalled(), 1);
    assert!(source.is_finished());
    assert_eq!(scheduler.task_count(), 0);
}

#[test]
fn dropping_scheduler_unwinds_suspended_task_stacks() {
    struct DropSignal(Rc<Cell<bool>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let dropped = Rc::new(Cell::new(false));
    let task_dropped = Rc::clone(&dropped);
    let mut scheduler = Scheduler::new(STACK_SIZE);
    scheduler
        .spawn(async move {
            let _drop_signal = DropSignal(task_dropped);
            Scheduler::block_current();
        })
        .expect("task stack should be allocated");

    assert_eq!(scheduler.run_until_stalled(), 1);
    assert!(!dropped.get());

    drop(scheduler);

    assert!(dropped.get());
}

#[test]
fn dropping_scheduler_unwinds_tasks_suspended_in_transfers() {
    struct DropSignal(Rc<Cell<bool>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let first_task_context = Rc::new(std::cell::RefCell::new(None::<Task>));
    let second_task_context = Rc::new(std::cell::RefCell::new(None::<Task>));
    let first_dropped = Rc::new(Cell::new(false));
    let second_dropped = Rc::new(Cell::new(false));
    let mut scheduler = Scheduler::new(STACK_SIZE);

    let first_context_slot = Rc::clone(&first_task_context);
    let second_context_slot = Rc::clone(&second_task_context);
    let second_drop_signal = Rc::clone(&second_dropped);
    scheduler
        .spawn(async move {
            let _drop_signal = DropSignal(second_drop_signal);
            let current = Task::current().expect("second task should have a context");
            *second_context_slot.borrow_mut() = Some(current.clone());
            current.block();

            let first = first_context_slot
                .borrow()
                .as_ref()
                .expect("first task should have started")
                .clone();
            current.transfer_to(&first);
        })
        .expect("second task stack should be allocated");

    let first_context_slot = Rc::clone(&first_task_context);
    let second_context_slot = Rc::clone(&second_task_context);
    let first_drop_signal = Rc::clone(&first_dropped);
    scheduler
        .spawn(async move {
            let _drop_signal = DropSignal(first_drop_signal);
            let current = Task::current().expect("first task should have a context");
            *first_context_slot.borrow_mut() = Some(current.clone());

            let second = second_context_slot
                .borrow()
                .as_ref()
                .expect("second task should have started")
                .clone();
            current.transfer_to(&second);
            current.block();
        })
        .expect("first task stack should be allocated");

    assert_eq!(scheduler.run_until_stalled(), 2);
    assert!(!first_dropped.get());
    assert!(!second_dropped.get());

    drop(scheduler);

    assert!(first_dropped.get());
    assert!(second_dropped.get());
}
