//! Foundational concurrency APIs for Socketry.

#![doc = include_str!("../readme.md")]

pub use socketry_executor as executor;
pub use socketry_executor::{
    Barrier, BufferResult, Clock, FileIo, Interest, Network, Scheduler, SchedulerHandle, Spawn,
    SpawnError, Task, TaskError, TaskHandle, yield_now,
};

pub use socketry_executor::scheduler;
