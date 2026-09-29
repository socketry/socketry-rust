//! Foundational concurrency APIs for Socketry.

#![doc = include_str!("../readme.md")]

pub use socketry_concurrent as concurrent;
pub use socketry_concurrent::{Pool, Scheduler, SchedulerHandle, Stack, Task, TaskHandle, wait};
