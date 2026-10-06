// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

//! Owned tasks and cooperative future polling on work-stealing workers.
//!
//! Tasks require `Send + 'static` and use the ordinary worker thread stack.
//! The scheduler owns spawned tasks even when their join handles are dropped.
//! Use a [`Barrier`] to explicitly own a group of child tasks.
#![doc = include_str!("../readme.md")]

mod barrier;
mod owner;
pub mod scheduler;
mod task;
mod worker;

pub use barrier::Barrier;
pub use owner::{Spawn, SpawnError};
pub use scheduler::{BufferResult, Clock, FileIo, Interest, Network, Scheduler, SchedulerHandle};
pub use task::{Task, TaskError, TaskHandle, yield_now};
