use crate::Stack;
use crate::context::Context;
use crate::coroutine::Coroutine;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const STACK_SIZE: usize = 64 * 1024;

struct DropSignal(Rc<Cell<bool>>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

fn dispatch(coroutine: &mut Coroutine, scheduler_context: &mut Context) {
    coroutine.dispatch(scheduler_context);
}

#[test]
fn task_context_is_current_only_while_its_stack_is_running() {
    assert!(Coroutine::current().is_none());

    let current = Rc::new(RefCell::new(None));
    let task_current = Rc::clone(&current);
    let mut task = Coroutine::new(STACK_SIZE, move || {
        *task_current.borrow_mut() = Coroutine::current();
        Coroutine::park();
    })
    .expect("task stack should be allocated");
    let mut scheduler_context = Context::empty();

    dispatch(&mut task, &mut scheduler_context);
    assert!(Coroutine::current().is_none());

    let task_handle = current
        .borrow()
        .as_ref()
        .expect("current task handle")
        .clone();
    assert!(!task_handle.is_finished());

    dispatch(&mut task, &mut scheduler_context);
    assert!(task_handle.is_finished());
}

#[test]
fn returning_from_the_task_unwinds_its_stack_before_returning_to_the_scheduler() {
    let dropped = Rc::new(Cell::new(false));
    let task_dropped = Rc::clone(&dropped);
    let mut task = Coroutine::new(STACK_SIZE, move || {
        let _drop_signal = DropSignal(task_dropped);
    })
    .expect("task stack should be allocated");
    let mut scheduler_context = Context::empty();

    dispatch(&mut task, &mut scheduler_context);

    assert!(dropped.get());
    assert!(task.is_finished());
}

#[test]
fn task_completion_preserves_a_nested_callers_stack() {
    let parent_frame_dropped = Rc::new(Cell::new(false));
    let child_frame_dropped = Rc::new(Cell::new(false));
    let returned_from_child = Rc::new(Cell::new(false));

    let task_parent_frame_dropped = Rc::clone(&parent_frame_dropped);
    let task_child_frame_dropped = Rc::clone(&child_frame_dropped);
    let task_returned_from_child = Rc::clone(&returned_from_child);
    let mut parent = Coroutine::new(STACK_SIZE, move || {
        let _parent_drop_signal = DropSignal(Rc::clone(&task_parent_frame_dropped));
        let child_dropped = Rc::clone(&task_child_frame_dropped);
        let mut child = Coroutine::new(STACK_SIZE, move || {
            let _child_drop_signal = DropSignal(child_dropped);
        })
        .expect("child task stack should be allocated");
        let mut child_scheduler_context = Context::empty();

        dispatch(&mut child, &mut child_scheduler_context);
        assert!(task_child_frame_dropped.get());
        assert!(!task_parent_frame_dropped.get());
        task_returned_from_child.set(true);
        Coroutine::park();
    })
    .expect("parent task stack should be allocated");
    let mut scheduler_context = Context::empty();

    dispatch(&mut parent, &mut scheduler_context);

    assert!(child_frame_dropped.get());
    assert!(returned_from_child.get());
    assert!(!parent_frame_dropped.get());

    dispatch(&mut parent, &mut scheduler_context);
    assert!(parent_frame_dropped.get());
    assert!(parent.is_finished());
}

#[test]
fn task_transfers_continue_at_each_tasks_own_suspension_point() {
    let a_handle = Rc::new(RefCell::new(None));
    let b_handle = Rc::new(RefCell::new(None));
    let events = Rc::new(RefCell::new(Vec::new()));

    let a_b_handle = Rc::clone(&b_handle);
    let a_events = Rc::clone(&events);
    let mut task_a = Coroutine::new(STACK_SIZE, move || {
        a_events.borrow_mut().push("a-start");
        Coroutine::transfer(a_b_handle.borrow().as_ref().expect("task B handle"));
        a_events.borrow_mut().push("a-after-first-transfer");
        Coroutine::transfer(a_b_handle.borrow().as_ref().expect("task B handle"));
        a_events.borrow_mut().push("a-after-second-transfer");
        Coroutine::park();
        a_events.borrow_mut().push("a-after-dispatch");
    })
    .expect("task A stack should be allocated");
    *a_handle.borrow_mut() = Some(task_a.handle());

    let b_a_handle = Rc::clone(&a_handle);
    let b_events = Rc::clone(&events);
    let mut task_b = Coroutine::new(STACK_SIZE, move || {
        b_events.borrow_mut().push("b-start");
        Coroutine::transfer(b_a_handle.borrow().as_ref().expect("task A handle"));
        b_events.borrow_mut().push("b-after-first-transfer");
        Coroutine::transfer(b_a_handle.borrow().as_ref().expect("task A handle"));
        b_events.borrow_mut().push("b-after-second-transfer");
        Coroutine::park();
        b_events.borrow_mut().push("b-after-dispatch");
    })
    .expect("task B stack should be allocated");
    *b_handle.borrow_mut() = Some(task_b.handle());
    let mut scheduler_context = Context::empty();

    dispatch(&mut task_a, &mut scheduler_context);

    assert_eq!(
        *events.borrow(),
        [
            "a-start",
            "b-start",
            "a-after-first-transfer",
            "b-after-first-transfer",
            "a-after-second-transfer",
        ]
    );
    assert!(!task_a.is_finished());
    assert!(!task_b.is_finished());

    dispatch(&mut task_b, &mut scheduler_context);
    assert_eq!(events.borrow().last(), Some(&"b-after-second-transfer"));
    assert!(!task_b.is_finished());

    dispatch(&mut task_b, &mut scheduler_context);
    assert_eq!(events.borrow().last(), Some(&"b-after-dispatch"));
    assert!(task_b.is_finished());

    dispatch(&mut task_a, &mut scheduler_context);
    assert_eq!(events.borrow().last(), Some(&"a-after-dispatch"));
    assert!(task_a.is_finished());
}

#[test]
fn transfer_source_resumes_only_after_scheduler_dispatch() {
    let b_handle = Rc::new(RefCell::new(None));
    let transfer_returned = Rc::new(Cell::new(false));

    let mut task_b = Coroutine::new(STACK_SIZE, || {
        Coroutine::park();
    })
    .expect("task B stack should be allocated");
    *b_handle.borrow_mut() = Some(task_b.handle());

    let a_b_handle = Rc::clone(&b_handle);
    let a_transfer_returned = Rc::clone(&transfer_returned);
    let mut task_a = Coroutine::new(STACK_SIZE, move || {
        Coroutine::transfer(a_b_handle.borrow().as_ref().expect("task B handle"));
        a_transfer_returned.set(true);
    })
    .expect("task A stack should be allocated");
    let mut scheduler_context = Context::empty();

    dispatch(&mut task_b, &mut scheduler_context);
    assert!(!task_b.is_finished());

    dispatch(&mut task_a, &mut scheduler_context);

    assert!(!transfer_returned.get());
    assert!(!task_a.is_finished());
    assert!(task_b.is_finished());

    dispatch(&mut task_a, &mut scheduler_context);

    assert!(transfer_returned.get());
    assert!(task_a.is_finished());
}

#[test]
fn returning_from_a_transferred_to_task_returns_to_the_scheduler() {
    let b_handle = Rc::new(RefCell::new(None));
    let transfer_panicked = Rc::new(Cell::new(false));
    let a_b_handle = Rc::clone(&b_handle);
    let a_transfer_panicked = Rc::clone(&transfer_panicked);
    let mut task_a = Coroutine::new(STACK_SIZE, move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Coroutine::transfer(a_b_handle.borrow().as_ref().expect("task B handle"));
        }));
        a_transfer_panicked.set(result.is_err());
    })
    .expect("task A stack should be allocated");

    let task_b = Coroutine::new(STACK_SIZE, || {}).expect("task B stack should be allocated");
    *b_handle.borrow_mut() = Some(task_b.handle());
    let mut scheduler_context = Context::empty();

    dispatch(&mut task_a, &mut scheduler_context);

    assert!(!transfer_panicked.get());
    assert!(!task_a.is_finished());
    assert!(task_b.is_finished());

    dispatch(&mut task_a, &mut scheduler_context);

    assert!(!transfer_panicked.get());
    assert!(task_a.is_finished());
}

#[test]
fn task_can_use_a_preallocated_stack() {
    let stack = Stack::new(STACK_SIZE).expect("stack should be allocated");
    let mut task = Coroutine::with_stack(stack, || {}).expect("task should use the supplied stack");
    let mut scheduler_context = Context::empty();

    dispatch(&mut task, &mut scheduler_context);

    assert!(task.is_finished());
}

#[test]
fn task_panics_propagate_to_the_scheduler() {
    let mut task = Coroutine::new(STACK_SIZE, || panic!("task entry failed"))
        .expect("task stack should be allocated");
    let mut scheduler_context = Context::empty();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch(&mut task, &mut scheduler_context)
    }));

    assert!(result.is_err());
    assert!(task.is_finished());
}
