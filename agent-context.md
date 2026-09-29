# Agent Context

This repository is the foundational Rust workspace for Socketry. Read
`conventions.md` before changing package names, documentation, or workspace
layout.

## Workspace

- `socketry` is the public facade and re-exports the common concurrency API.
- `socketry-concurrent` owns the coroutine implementation and the cooperative
  future scheduler. `Scheduler::spawn` accepts futures; `wait` lets ordinary
  synchronous functions suspend on futures using the current task's stack.
- Each workspace package is published independently to crates.io.

## Implementation boundaries

- The native context-switch assembly is vendored from CRuby. Its upstream
  revision and attribution are recorded in
  `crates/concurrent/vendor/cruby/upstream.md` and
  `crates/concurrent/vendor/cruby/license.md`.
- Suspended tasks are thread-affine. Do not migrate their stacks between
  worker threads.
- Future wakers may enqueue work from another thread; the owning scheduler
  resumes its tasks.
- A nested `wait` preserves an in-progress `Future::poll` call. Resume that
  stack before polling the outer future again. Preserve outer wakeups consumed
  while nested waits are active.
- Both nested waits and ordinary future suspension currently stay on the
  owning thread. Migration between completed polls is a possible future
  extension and is not implemented.
