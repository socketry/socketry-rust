use socketry_concurrent::{Scheduler, Task, wait};
use std::cell::{Cell, RefCell};
use std::future::{Future, pending, poll_fn, ready};
use std::marker::PhantomPinned;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};

const STACK_SIZE: usize = 64 * 1024;

#[derive(Clone, Default)]
struct Gate {
    opened: Rc<Cell<bool>>,
    waker: Rc<RefCell<Option<Waker>>>,
}

impl Gate {
    fn open(&self) {
        self.opened.set(true);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

impl Future for Gate {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.opened.get() {
            Poll::Ready(())
        } else {
            *self.waker.borrow_mut() = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

fn wake_once() -> impl Future<Output = ()> {
    let mut first_poll = true;
    poll_fn(move |context| {
        if first_poll {
            first_poll = false;
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
}

fn suspend_once() {
    wait(wake_once());
}

#[test]
fn an_async_task_can_mix_await_and_nested_wait() {
    let stages = Rc::new(Cell::new(0));
    let task_stages = Rc::clone(&stages);
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async move {
            wake_once().await;
            task_stages.set(1);
            suspend_once();
            task_stages.set(2);
            wake_once().await;
            task_stages.set(3);
        })
        .unwrap();

    assert_eq!(scheduler.run_until_stalled(), 4);
    assert_eq!(stages.get(), 3);
    assert!(task.is_finished());
}

#[test]
fn ordinary_nested_functions_can_wait_with_borrowed_state() {
    fn inner(message: &mut String) -> &str {
        wait(async {
            suspend_once();
            message.push_str(" world");
            message.as_str()
        })
    }

    fn outer(message: &mut String) -> &str {
        inner(message)
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let task_events = Rc::clone(&events);
    let mut scheduler = Scheduler::new(STACK_SIZE);
    scheduler
        .spawn(async move {
            task_events.borrow_mut().push("before wait");
            let mut message = String::from("hello");
            assert_eq!(outer(&mut message), "hello world");
            task_events.borrow_mut().push("after wait");
        })
        .unwrap();
    let task_events = Rc::clone(&events);
    scheduler
        .spawn(async move {
            task_events.borrow_mut().push("other task");
        })
        .unwrap();

    scheduler.run_until_stalled();
    assert_eq!(
        *events.borrow(),
        ["before wait", "other task", "after wait"]
    );
    assert_eq!(scheduler.task_count(), 0);
}

#[test]
fn nested_wait_resumes_the_existing_poll_before_polling_again() {
    let polls = Rc::new(Cell::new(0));
    let returned_from_wait = Rc::new(Cell::new(false));
    let gate = Gate::default();
    let task_gate = gate.clone();
    let task_polls = Rc::clone(&polls);
    let task_returned = Rc::clone(&returned_from_wait);
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(poll_fn(move |context| {
            task_polls.set(task_polls.get() + 1);
            if task_polls.get() == 1 {
                wait(task_gate.clone());
                task_returned.set(true);
                context.waker().wake_by_ref();
                Poll::Pending
            } else {
                assert!(task_returned.get());
                Poll::Ready(())
            }
        }))
        .unwrap();

    assert_eq!(scheduler.run_until_stalled(), 1);
    // Even a spurious wake must continue the nested wait, not enter poll again.
    task.unblock();
    assert_eq!(scheduler.run_until_stalled(), 1);
    assert_eq!(polls.get(), 1);
    assert!(!returned_from_wait.get());

    gate.open();
    scheduler.run_until_stalled();
    assert_eq!(polls.get(), 2);
    assert!(task.is_finished());
}

#[test]
fn outer_wake_survives_being_consumed_by_a_nested_wait() {
    let outer_waker = Rc::new(RefCell::new(None::<Waker>));
    let task_waker = Rc::clone(&outer_waker);
    let gate = Gate::default();
    let task_gate = gate.clone();
    let mut first_poll = true;
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(poll_fn(move |context| {
            if first_poll {
                first_poll = false;
                *task_waker.borrow_mut() = Some(context.waker().clone());
                // Two levels of wait must preserve the outer notification.
                wait(async { wait(task_gate.clone()) });
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }))
        .unwrap();

    assert_eq!(scheduler.run_until_stalled(), 1);
    outer_waker.borrow().as_ref().unwrap().wake_by_ref();
    assert_eq!(scheduler.run_until_stalled(), 1);
    assert!(!task.is_finished());

    gate.open();
    scheduler.run_until_stalled();
    assert!(task.is_finished(), "the outer wake must cause another poll");
}

#[test]
fn a_wake_before_entering_wait_is_preserved() {
    let gate = Gate::default();
    let task_gate = gate.clone();
    let mut first_poll = true;
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(poll_fn(move |context| {
            if first_poll {
                first_poll = false;
                context.waker().wake_by_ref();
                wait(task_gate.clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }))
        .unwrap();

    scheduler.run_until_stalled();
    gate.open();
    scheduler.run_until_stalled();
    assert!(task.is_finished());
}

#[test]
fn caught_nested_panics_preserve_outer_notifications() {
    let mut first_poll = true;
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(poll_fn(move |_| {
            if first_poll {
                first_poll = false;
                let result = catch_unwind(|| {
                    wait(async {
                        suspend_once();
                        panic!("nested failure");
                    });
                });
                assert!(result.is_err());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }))
        .unwrap();

    scheduler.run_until_stalled();
    assert!(task.is_finished());
}

#[test]
fn nested_wait_accepts_a_non_unpin_future_without_moving_it() {
    struct AddressCheck {
        address: Cell<*const AddressCheck>,
        _pin: PhantomPinned,
    }

    impl Future for AddressCheck {
        type Output = usize;

        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<usize> {
            let this = self.as_ref().get_ref();
            if this.address.get().is_null() {
                this.address.set(std::ptr::from_ref(this));
                context.waker().wake_by_ref();
                Poll::Pending
            } else {
                assert_eq!(this.address.get(), std::ptr::from_ref(this));
                Poll::Ready(42)
            }
        }
    }

    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async {
            assert_eq!(
                wait(AddressCheck {
                    address: Cell::new(std::ptr::null()),
                    _pin: PhantomPinned,
                }),
                42
            );
        })
        .unwrap();

    scheduler.run_until_stalled();
    assert!(task.is_finished());
}

#[test]
fn a_future_can_wake_a_nested_wait_from_another_thread() {
    let owner = std::thread::current().id();
    let completed = Arc::new(AtomicBool::new(false));
    let producer_completed = Arc::clone(&completed);
    let (sender, receiver) = mpsc::channel::<Waker>();
    let producer = std::thread::spawn(move || {
        let waker = receiver.recv().unwrap();
        producer_completed.store(true, Ordering::Release);
        waker.wake();
    });
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async move {
            let mut sender = Some(sender);
            let result = wait(poll_fn(|context| {
                assert_eq!(std::thread::current().id(), owner);
                if completed.load(Ordering::Acquire) {
                    Poll::Ready(123)
                } else {
                    if let Some(sender) = sender.take() {
                        sender.send(context.waker().clone()).unwrap();
                    }
                    Poll::Pending
                }
            }));
            assert_eq!(result, 123);
            assert_eq!(std::thread::current().id(), owner);
        })
        .unwrap();

    scheduler.run();
    producer.join().unwrap();
    assert!(task.is_finished());
}

#[test]
fn dropping_scheduler_unwinds_all_nested_wait_frames() {
    struct DropRecord(&'static str, Rc<RefCell<Vec<&'static str>>>);

    impl Drop for DropRecord {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }

    let drops = Rc::new(RefCell::new(Vec::new()));
    let task_drops = Rc::clone(&drops);
    let saved_waker = Rc::new(RefCell::new(None));
    let task_waker = Rc::clone(&saved_waker);
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async move {
            let _outer = DropRecord("outer", Rc::clone(&task_drops));
            wait(async {
                let _inner = DropRecord("inner", Rc::clone(&task_drops));
                wait(poll_fn(|context| {
                    *task_waker.borrow_mut() = Some(context.waker().clone());
                    Poll::<()>::Pending
                }));
            });
        })
        .unwrap();

    scheduler.run_until_stalled();
    assert!(drops.borrow().is_empty());
    drop(scheduler);
    assert_eq!(*drops.borrow(), ["inner", "outer"]);
    assert!(task.is_finished());
    saved_waker.borrow_mut().take().unwrap().wake();
    assert!(Scheduler::current().is_none());
    assert!(Task::current().is_none());
}

#[test]
fn nested_future_panics_reach_the_scheduler_and_recycle_the_stack() {
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async {
            wait(async {
                suspend_once();
                panic!("nested future failed");
            });
        })
        .unwrap();

    let result = catch_unwind(AssertUnwindSafe(|| scheduler.run_until_stalled()));
    let payload = result.unwrap_err();
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"nested future failed")
    );
    assert!(task.is_finished());
    assert_eq!(scheduler.task_count(), 0);
    assert_eq!(scheduler.cached_stacks(), 1);
    assert!(Scheduler::current().is_none());
}

#[test]
fn completed_task_wakers_do_not_wake_a_reused_stack() {
    let saved_waker = Rc::new(RefCell::new(None));
    let task_waker = Rc::clone(&saved_waker);
    let mut scheduler = Scheduler::new(STACK_SIZE);
    scheduler
        .spawn(async move {
            wait(poll_fn(|context| {
                *task_waker.borrow_mut() = Some(context.waker().clone());
                Poll::Ready(())
            }));
        })
        .unwrap();
    scheduler.run_until_stalled();
    assert_eq!(scheduler.cached_stacks(), 1);

    let task = scheduler.spawn(async { wait(pending::<()>()) }).unwrap();
    assert_eq!(scheduler.cached_stacks(), 0);
    scheduler.run_until_stalled();
    saved_waker.borrow_mut().take().unwrap().wake();
    assert_eq!(scheduler.run_until_stalled(), 0);
    assert!(!task.is_finished());
}

#[test]
fn scheduler_handle_wait_uses_the_current_task() {
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async {
            let scheduler = Scheduler::current().unwrap();
            let answer = scheduler.wait(async {
                suspend_once();
                42
            });
            assert_eq!(answer, 42);
        })
        .unwrap();
    scheduler.run_until_stalled();
    assert!(task.is_finished());
}

#[test]
#[should_panic(expected = "cannot wait outside a scheduler task")]
fn wait_rejects_calls_outside_a_task() {
    wait(ready(()));
}

#[test]
fn scheduler_handle_wait_rejects_a_different_scheduler() {
    let other = Scheduler::new(STACK_SIZE);
    let handle = other.handle();
    let mut scheduler = Scheduler::new(STACK_SIZE);
    let task = scheduler
        .spawn(async move {
            handle.wait(ready(()));
        })
        .unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| scheduler.run_until_stalled()));
    assert_eq!(
        result.unwrap_err().downcast_ref::<&str>(),
        Some(&"cannot wait using a different scheduler")
    );
    assert!(task.is_finished());
}
