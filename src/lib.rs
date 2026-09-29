//! Foundational concurrency APIs for Socketry.

#![doc = include_str!("../readme.md")]

pub use socketry_concurrent as concurrent;
pub use socketry_concurrent::{
	Fiber, Pool, Scheduler, SchedulerHandle, Stack, TaskHandle,
};
