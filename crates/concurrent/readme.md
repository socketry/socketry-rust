# socketry-concurrent

Stackful fibers and cooperative scheduling for Socketry's Rust packages. Use
the `socketry` package for the one-stop public entry point, or depend on this
package directly to use the concurrency implementation on its own.

The API starts with three pieces:

- Stack reserves memory with inaccessible guard pages at both ends.
- Fiber owns a stack and switches between its caller and a closure.
- Pool keeps multiple stacks available for reuse.

Scheduler is an optional, single-threaded executor. It can run Rust futures and
also exposes block_current and task unblock operations for stackful code.
Future wakers may run on any thread; the fiber itself always resumes on the
thread that owns its stack. Arbitrary Rust locals on a suspended stack are not
tracked by the type system, so moving a suspended fiber between workers is not
safe. The scheduler does not migrate fibers.

## Example

    use socketry_concurrent::Scheduler;

    fn main() -> std::io::Result<()> {
        let mut scheduler = Scheduler::new(256 * 1024);
        scheduler.spawn(async {
            // Await ordinary Rust futures here.
        })?;
        scheduler.run();
        Ok(())
    }

Fiber::yield_now returns control to the caller. Resuming that fiber continues
after the yield. Dropping a suspended fiber resumes it with a private
cancellation panic so Rust unwinds the stack and runs local destructors.

Use Scheduler::spawn_fiber for synchronous stackful tasks. Such a task can call
Scheduler::block_current and be made runnable again through its TaskHandle.

## Native context switches

Architecture-specific assembly is vendored from CRuby's coroutine directory.
The build currently integrates the x86-64 and AArch64 implementations on Linux,
macOS, and FreeBSD. Other CRuby assembly implementations are preserved in
vendor/cruby/coroutine but are not yet selected by this crate's build.

The x86-64 Linux build includes CRuby's CET shadow-stack switch path. It checks
whether shadow stacks are enabled at runtime and allocates a shadow stack for
each fiber only when needed.

The upstream source commit is recorded in `vendor/cruby/upstream.md`. The
vendored coroutine assembly is available under the MIT license in
vendor/cruby/license.md, including attribution for source files without an
embedded header.

## Current boundaries

- Fiber and Scheduler are thread-affine and are not Send.
- Scheduler tasks may contain non-Send futures because they are polled only on
  the scheduler's owning thread.
- TaskHandle::unblock and future wakers are thread-safe and only enqueue work;
  they never resume a fiber on the waking thread.
- This first version does not implement work stealing or cross-thread stack
  migration.
