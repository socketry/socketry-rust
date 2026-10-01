# Conventions

- Keep shared Rust guidance in the `bake-agent-context` package; this file records workspace-specific boundaries.
- Keep `socketry` as the facade and publish component packages with the `socketry-` prefix.
- Keep `socketry-executor` in `crates/executor`. Split other packages only when they have useful code and a clear dependency boundary.
- New concurrency work uses ordinary futures, not a private coroutine stack per task. The coroutine prototype remains on branch `coroutine`.
- Keep native I/O backends under `scheduler/selector/` and runtime adapters alongside their runtime implementation.
- Pass task owners explicitly when libraries spawn child tasks. Preserve barrier ownership, cancellation, and shutdown behavior.
- Keep public implementation guidance in `context/implementation.md` and architecture decisions in `context/design.md`.
- Keep repository-only conventions under `.agents/`; use the Bake Cargo agent context for the shared publishing process.
