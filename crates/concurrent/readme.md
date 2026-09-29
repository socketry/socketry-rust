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
each fiber only when needed.

## Sanitizers

The `address-sanitizer` and `thread-sanitizer` Cargo features enable the
compiler runtime's fiber-switch hooks. Pair one feature with the matching Rust
sanitizer flag on nightly; the hooks let each runtime follow the custom stacks
used by Fiber.

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

- Fiber and Scheduler are thread-affine and are not Send.
- Scheduler tasks may contain non-Send futures because they are polled only on
  the scheduler's owning thread.
- TaskHandle::unblock and future wakers are thread-safe and only enqueue work;
  they never resume a fiber on the waking thread.
- This first version does not implement work stealing or cross-thread stack
  migration.
