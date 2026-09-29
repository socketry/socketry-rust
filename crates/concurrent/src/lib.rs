//! Futures with nested stackful waits, guarded stacks, and cooperative scheduling.

mod context;
mod coroutine;
mod pool;
mod scheduler;
mod stack;

pub use pool::Pool;
pub use scheduler::{Scheduler, SchedulerHandle, Task, TaskHandle, wait};
pub use stack::Stack;

#[cfg(test)]
mod coroutine_tests;
