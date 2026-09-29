use socketry_concurrent::{Fiber, Stack};
use std::cell::Cell;
use std::rc::Rc;

const STACK_SIZE: usize = 64 * 1024;

#[test]
fn fiber_resumes_after_each_yield_and_finishes_once() {
    let steps = Rc::new(Cell::new(0));
    let fiber_steps = Rc::clone(&steps);
    let mut fiber = Fiber::new(STACK_SIZE, move || {
        fiber_steps.set(fiber_steps.get() + 1);
        Fiber::yield_now();

        fiber_steps.set(fiber_steps.get() + 1);
        Fiber::yield_now();

        fiber_steps.set(fiber_steps.get() + 1);
    })
    .expect("fiber stack should be allocated");

    assert_eq!(steps.get(), 0);
    assert!(!fiber.is_finished());

    fiber.resume();
    assert_eq!(steps.get(), 1);
    assert!(!fiber.is_finished());

    fiber.resume();
    assert_eq!(steps.get(), 2);
    assert!(!fiber.is_finished());

    fiber.resume();
    assert_eq!(steps.get(), 3);
    assert!(fiber.is_finished());

    fiber.resume();
    assert_eq!(steps.get(), 3);
}

#[test]
fn dropping_a_suspended_fiber_unwinds_its_stack() {
    struct DropSignal(Rc<Cell<bool>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let dropped = Rc::new(Cell::new(false));
    let fiber_dropped = Rc::clone(&dropped);
    let mut fiber = Fiber::new(STACK_SIZE, move || {
        let _drop_signal = DropSignal(fiber_dropped);
        Fiber::yield_now();
    })
    .expect("fiber stack should be allocated");

    fiber.resume();
    assert!(!fiber.is_finished());
    assert!(!dropped.get());

    drop(fiber);
    assert!(dropped.get());
}

#[test]
fn fiber_can_use_a_preallocated_stack() {
    let stack = Stack::new(STACK_SIZE).expect("stack should be allocated");
    let mut fiber = Fiber::with_stack(stack, || {}).expect("fiber should use the supplied stack");

    fiber.resume();

    assert!(fiber.is_finished());
}

#[test]
fn fiber_propagates_entry_panics_to_its_resumer() {
    let mut fiber = Fiber::new(STACK_SIZE, || panic!("fiber entry failed"))
        .expect("fiber stack should be allocated");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fiber.resume()));

    assert!(result.is_err());
    assert!(fiber.is_finished());
}
