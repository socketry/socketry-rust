# Agent Context

Read conventions.md, design.md and context/rust.md before changing public APIs,
runtime boundaries, package names or workspace layout.

## Implemented foundation

- `socketry` is the facade and packages the shared conventions and Rust context.
- `socketry-executor` executes futures using async-task and Crossbeam queues.
- Scheduler construction starts worker threads. Tasks require Send + 'static;
  the pinned future remains stationary while workers may change between polls.
- Workers own FIFO ready queues and concurrent incoming queues. Wakeups target
  the last worker; idle workers steal from ready queues and incoming queues.
- A shared injector accepts submissions originating outside a worker.
- Worker parking publishes an idle flag/count before rechecking all work.
  Preserve this protocol and treat Steal::Retry differently from empty.
- Queue operations use Crossbeam synchronization. Task admission, completion,
  cancellation and closure use a separate ownership registry mutex.
- Never run user code or invoke wakers while holding the registry mutex.
- Task context is installed around execution and destruction and restored on
  unwinding. Scheduler context is installed on workers and block_on roots.
- No coroutine stacks, nested synchronous wait or explicit transfer remain in
  the current executor. The full prototype is on branch coroutine at b520f3d.

## Ownership

- Scheduler and Barrier implement Spawn, with a runtime-specific associated
  handle type. Adapters need to preserve its ownership and result contract.
- Register ownership before publishing a runnable task.
- Dropping TaskHandle abandons the result without cancelling the owner's task.
- Task cancellation sets a flag and wakes it. The future is destroyed after
  an in-progress poll returns. TaskHandle::cancel awaits that destruction.
- Barrier::close prevents admission; wait awaits direct children; stop closes,
  cancels and waits. Drop requests cancellation without waiting.
- Awaited task results report cancellation or the original panic payload.
  Barrier waits and Scheduler::run do not aggregate child errors.
- Parents must explicitly await barriers for joined cleanup. Automatic waiting
  for descendants after parent destruction is not implemented.
- Scheduler Drop cancels all tasks, joining threads outside a worker. On a
  worker it requests shutdown without joining. Surviving handles reject spawn.

## I/O and runtime boundaries

- `scheduler/mod.rs` defines Network, FileIo and Clock. Operations return
  concrete Send futures; portable consumers receive the required capabilities.
- `scheduler/socketry.rs` owns the executor; `socketry/operations.rs` forwards
  capabilities to its lazily initialized, compile-time selected selector.
- `scheduler/selector/` contains readiness, epoll, kqueue, iocp and io_uring.
  Platform readiness modules share async-io's persistent registrations and
  process-wide reactor. Registered sockets remain usable as tasks migrate.
- Default feature `native` provides TCP, positioned files and sleep. Feature
  `io-uring` selects Linux completion reads/writes; other supported platforms
  retain readiness. Feature `tokio` enables the separate runtime adapter.
  No default features builds the executor and contracts without native I/O.
- Read/write buffers are owned Vec values, returned with ordinary errors as
  well as success. Reads use the initialized length and leave it unchanged.
  Operations can be partial; cancellation can consume or transmit bytes.
- io_uring's dedicated selector thread owns buffers and descriptors until the
  original terminal completion, never merely a cancellation completion. Numeric
  identifiers are not reused. Pending, unsubmitted requests can return buffers
  immediately. Unexpected failure retains kernel-accessible resources and
  panics waiting operations; do not free buffers with unknown completion.
- Ring shutdown closes admission, cancels and drains completions. Scheduler
  Drop outside a worker waits for this drain; Drop on a worker only requests it.
  Shutdown racing lazy initialization must still close the new selector.
- Regular files use blocking pools except with io_uring. An already-started
  blocking operation can outlive its waiting task. Use non-append regular files
  and explicit offsets; the Windows fallback also changes the shared cursor.
- `scheduler/tokio.rs` adapts an existing runtime with I/O and time enabled.
  It preserves Spawn/barrier ownership, but does not own or drive that runtime.
  Its shutdown joins owned task destruction, not the runtime's blocking pool.
- Register Tokio task ownership before spawn, but release the registry mutex
  before calling into Tokio: a stopped runtime may synchronously drop a future.
  Future destruction precedes ownership completion. Never hold an EnterGuard
  across await; enter per poll for operations that register resources.
- Tokio resources carry runtime identity. Explicit handles select that runtime;
  Socketry's Task::current and Scheduler::current remain Socketry-specific.

## Next boundaries

- The io-event timer port, operation pools, registered buffers, general
  descriptor/UDP interfaces and overlapped Windows file operations remain
  planned. Native sleep currently uses async-io; the adapter uses Tokio timers.
- Keep runtime requirements explicit; ordinary Future support does not provide
  another runtime's I/O or timers.
- There is no non-Send task executor, public blocking-task API or scoped borrowing.
- Do not introduce unsafe Send, stack migration or nested blocking waits.
- Native coroutine sources, sanitizer hooks and historical tests belong to
  the preserved coroutine branch, not the future executor's build.

## Verification

Public behavior is covered in crates/executor/tests. Deterministic channels
force stealing, migration, concurrent wakeups and cancellation during polling.
The parking test uses Loom to model the queue-publication/idle-registration
handshake. Keep its atomics and fence order aligned with the implementation;
this models the handshake, not Crossbeam or async-task internals.
When verification is requested, run workspace tests and doctests with `tokio`
enabled; on Linux also run all features to exercise io_uring. Check executor-only
and Tokio-only feature combinations. The same TCP/file consumers exercise both
implementations; Linux tests cover cancellation batches and shutdown races.
CI covers Linux, macOS, Windows and FreeBSD, with separate Linux io_uring and
sanitizer jobs; distinguish configured CI from executed results.
