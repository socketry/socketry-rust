# Rust agent context

Use this guidance when working on Socketry's Rust crates. Read the current
repository's conventions.md and agent-context.md first. Those files establish
project-specific choices and which capabilities are actually implemented.

## Working from the Ruby designs

Port behavior, ownership relationships, modularity, and performance choices.
Do not translate Ruby syntax mechanically or introduce a hierarchy of traits
solely to reproduce class inheritance. Prefer data types and composition for
shared state, and traits for capabilities that real consumers need.

Keep protocol representation and parsing separate from transport and task
scheduling. A protocol library should accept the I/O or owner it needs rather
than silently choosing a runtime for the application.

Record the exact upstream revision and relevant files for a port. Preserve
license notices and contributor attribution. Explain intentional semantic
differences, especially ownership, cancellation, error handling, and time.

## Futures and executors

- Calling an async function constructs a future. Its body runs when polled.
- Poll::Pending returns control to the executor. The future must arrange a
  wakeup when it can make progress, and update a stored waker when necessary.
- Treat a wakeup as a request to poll, not proof that the operation is ready.
  Preserve wakeups that race with polling or queue transitions.
- Polling must not synchronously block a worker. CPU-intensive work and
  blocking system calls need a deliberate execution strategy.
- Do not poll a task concurrently or poll a completed future again.
- Pin preserves an address; Send permits transfer between threads. Neither
  implies the other. A future can be Send without being Unpin.
- A Send future can contain non-Send local values during a poll when those
  values are not retained across its suspension points. Suspending an
  ordinary call stack inside poll invalidates attempts to infer stack
  migration safety from the outer future's Send bound.
- The future foundation uses ordinary polling and `.await`. Do not restore
  per-task stacks or hidden nested block_on calls as a compatibility shortcut.
- Concrete I/O and timer futures can require particular runtime services even
  though they implement the standard Future trait. Trait adaptation does not
  replace those services. Call Socketry's native backends selectors; dependencies
  such as Tokio may use their own terminology.

## Tasks and cleanup

Keep spawning, ownership, execution, cancellation, and completion distinct in
the implementation. An explicit owner controls a child task's lifetime; a
worker polls its future. Libraries spawning children should receive an owner.

Document what happens when a handle is dropped. A request to stop is different
from confirmation that the task and its children have stopped. Use an awaited
shutdown operation when callers need that confirmation.

On stable Rust, Drop cannot await. When a future is dropped, ordinary
destructors release retained state, but code after its current await is not
executed. If cleanup needs async work, the runtime must retain and poll the
cleanup work, or the caller must explicitly await it before dropping resources.

Do not use a destructor alone to justify tasks borrowing their owner's locals.
Account for cancellation, early returns, panics, forgotten futures, and work
already running on another thread before introducing scoped borrowing.

## I/O and timers

For completion-based I/O, keep buffers and descriptors alive until the selector
has established the kernel no longer accesses them. A cancellation request or
a dropped waiting future is not that confirmation. Prefer owned buffers in the
lowest-level operation API, and document any copy introduced by stream adapters.

Reuse stream registration and operation storage where practical. When storage
is reused, ensure a stale completion, waker, or cancellation cannot affect the
new occupant. Invoke user code and wakeups outside selector locks.

Select concrete implementations with traits, target configuration and Cargo
features. Keep completion operations and readiness fallbacks behind the same
portable contract where the semantics match. Ordinary files need a blocking
pool or completion API; readiness alone does not make their operations async.

When adapting another runtime, document who keeps it alive and drives events.
Enter its context for each relevant poll, and restore context before returning
Pending. A runtime context guard must not survive across suspension or migration.
Keep registrations attached to their originating runtime and validate resource
identity where different runtime instances are incompatible.

Use monotonic clocks for deadlines. Keep timer ordering independent of the
runtime, with explicit time inputs suitable for deterministic verification.
Distinguish elapsed deadlines, clock overflow, and cancellation from I/O errors.

## Implementing changes

- Inspect existing code and repository status before editing; preserve work
  outside the requested change.
- Prefer standard Cargo layout, rustfmt, and readable names. Use lowercase
  Markdown documents and start license.md with `# MIT License`.
- Make runtime dependencies optional at the right package boundary. Keep
  portable interfaces buildable without the native selector.
- Check generic bounds at public trait boundaries, including whether returned
  futures need to be Send. Avoid introducing a Box per operation just to make
  a trait object convenient.
- Keep unsafe operations small and state the lifetime, aliasing, pinning, and
  thread assumptions they depend on. Use Result for expected failures.
- When verification is requested, cover the behavior being changed using
  deterministic synchronization and time. Run platform-dependent operations
  on the corresponding platform and report what actually ran.
- Make benchmarks establish allocation counts, contention, and throughput
  claims; do not infer a speedup merely from a language change.
- Update documentation and agent context when changing public behavior.
  Clearly label proposed APIs that do not yet exist.
