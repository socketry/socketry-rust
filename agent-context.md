# Agent Context

This repository is the foundational Rust workspace for Socketry. Read
`conventions.md` before changing package names, documentation, or workspace
layout.

## Workspace

- `socketry` is the public facade and re-exports the common concurrency API.
- `socketry-concurrent` owns the stackful fiber implementation and the
  cooperative scheduler.
- Each workspace package is published independently to crates.io.

## Implementation boundaries

- The native context-switch assembly is vendored from CRuby. Its upstream
  revision and attribution are recorded in
  `crates/concurrent/vendor/cruby/upstream.md` and
  `crates/concurrent/vendor/cruby/license.md`.
- Suspended fibers are thread-affine. Do not migrate their stacks between
  worker threads.
- Future wakers may enqueue work from another thread; the owning scheduler
  resumes its fibers.
