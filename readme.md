# socketry

Foundational concurrency APIs for Rust projects in the Socketry ecosystem.

This package is the convenient entry point. It re-exports the focused
`socketry-concurrent` package, which runs futures on coroutine stacks so
ordinary synchronous functions can wait for asynchronous operations.

## Usage

```toml
[dependencies]
socketry = "0.1"
```

```rust
use socketry::{Scheduler, wait};
use std::future::poll_fn;
use std::task::Poll;

fn answer() -> usize {
	let mut first_poll = true;
	wait(poll_fn(|context| {
		if first_poll {
			first_poll = false;
			context.waker().wake_by_ref();
			Poll::Pending
		} else {
			Poll::Ready(42)
		}
	}))
}

fn main() -> std::io::Result<()> {
	let mut scheduler = Scheduler::new(256 * 1024);
	scheduler.spawn(async {
		assert_eq!(answer(), 42);
	})?;
	scheduler.run();
	Ok(())
}
```

`answer` is an ordinary function. When its future is pending, `wait` suspends
the current task's stack and lets other tasks run. Use `.await` and nested
`wait` calls together as needed. `Scheduler::current()` also returns a handle
with a `wait` method.

The current scheduler runs on one OS thread and supports non-Send futures.
Wakers can run on other threads; suspended stacks resume on their owning
thread. It does not yet implement migration or work stealing.

The detailed implementation notes and platform support are documented in the
[`socketry-concurrent` package](https://github.com/socketry/socketry-rust/tree/main/crates/concurrent).
