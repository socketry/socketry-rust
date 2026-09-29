# socketry-concurrent

Futures with nested stackful waits for Socketry's Rust packages. Use
the `socketry` package for the one-stop public entry point, or depend on this
package directly to use the concurrency implementation on its own.

The library provides three building blocks:

- `Stack` reserves memory with inaccessible guard pages at both ends.
- `Pool` keeps multiple stacks available for reuse.
- `Scheduler` accepts futures and polls each task on its own coroutine stack.

`wait(future)` lets an ordinary function wait for an asynchronous result. When
the future is pending, the task's stack is suspended and the executor can run
other tasks. The function returns the future's output after it completes. Calls
can be nested, including inside another future's `poll`, without making the
calling functions async.

`Task::current` provides a thread-affine reference for blocking and task-to-task
transfer. `Scheduler::spawn` returns a separate, thread-safe `TaskHandle` for
waking the task. Future wakers may run on any thread; each task always resumes
on the thread that owns its stack.

## Example

    use socketry_concurrent::{Scheduler, wait};
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

`Scheduler::current().unwrap().wait(future)` is also available. Both forms
require a current scheduler task. A future can borrow local data and does not
need to implement Send or Unpin. Each wait pins the future on the task stack
and reuses the task's wake signal, without allocating a separate future or
waker. A future from another runtime still needs that runtime's I/O, timer,
or other services to be running.

Run the producer/consumer example with:

    cargo run --package socketry-concurrent --example nested_wait

Use `Task::current().unwrap().block()` or `Scheduler::block_current()` to park a
task. Another thread can make it runnable through its `TaskHandle`. Dropping a
scheduler unwinds suspended task stacks and runs their local destructors.

## Polling and nested waits

An ordinary `.await` can return `Poll::Pending` from the task's future. A nested
`wait`, however, can suspend while that same `poll` call is still executing.
The scheduler resumes the saved stack at that wait; it does not call the outer
future's `poll` again until the previous invocation has returned. Wakeups for
outer futures are preserved while inner waits run.

This implementation uses one scheduler thread for both cases. A future
executor could move a Send future between completed polls, but a stack
suspended inside a poll must remain on its original worker. The current
scheduler implements neither work stealing nor cross-thread migration.

## Native context switches

The complete CRuby coroutine source tree is vendored, including all platform
backends, helper implementations, and upstream tests. The build integrates
x86-64, x86, AArch64,
32-bit ARM, RISC-V64, LoongArch64, and PowerPC variants on the Unix targets
supported by each upstream backend. The PowerPC 32-bit and big-endian 64-bit
backends are Darwin-only upstream; PowerPC64 little-endian is Linux-only.

The selected upstream `Context.S` file provides register switching. Its
matching `Context.h` stack initialization logic is ported into separate Rust
modules under `src/context/`; the original headers remain available in the
vendored tree for reference.

The x86-64 Linux build includes CRuby's CET shadow-stack switch path. It checks
whether shadow stacks are enabled at runtime and allocates a shadow stack for
each task only when needed.

## Sanitizers

The `address-sanitizer` and `thread-sanitizer` Cargo features enable the
compiler runtime's fiber-switch hooks. Pair one feature with the matching Rust
sanitizer flag on nightly; the hooks let each runtime follow the custom stacks
used by scheduled tasks.

    RUSTFLAGS="-Zsanitizer=address" cargo +nightly test -Zbuild-std --target aarch64-apple-darwin --package socketry-concurrent --features address-sanitizer

    RUSTFLAGS="-Zsanitizer=thread" cargo +nightly test -Zbuild-std --target aarch64-apple-darwin --package socketry-concurrent --features thread-sanitizer

Install the `rust-src` component for nightly before using `-Zbuild-std`. The
Rust sanitizer flags and supported targets are documented in the
[Rust Unstable Book](https://doc.rust-lang.org/unstable-book/compiler-flags/sanitizer.html).
AddressSanitizer and ThreadSanitizer cannot be enabled together.

The upstream source commit is recorded in `vendor/cruby/upstream.md`. The
vendored coroutine sources are available under the MIT license in
vendor/cruby/license.md, including attribution for source files without an
embedded header.

## Current boundaries

- Task contexts and Scheduler are thread-affine and are not Send.
- Scheduler tasks may contain non-Send futures because they are polled only on
  the scheduler's owning thread.
- TaskHandle::unblock and future wakers are thread-safe and only enqueue work;
  they never resume a task on the waking thread.
- This first version does not implement work stealing or cross-thread stack
  migration.
