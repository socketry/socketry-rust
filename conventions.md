# Conventions

## Repository and packages

- Use lowercase filenames for Markdown documents. In particular, use
  readme.md and license.md.
- The first line of each license.md is `# MIT License`. Preserve attribution
  when copying or porting upstream code.
- Keep foundational Rust packages in this repository's Cargo workspace. Give
  published component packages the `socketry-` prefix.
- Keep the root Cargo package named `socketry`; use it as the convenient public
  facade for the foundational packages.
- Keep package APIs in separately publishable workspace members when they have
  a clear dependency and version boundary.
- Keep this file as the record of project-specific conventions and follow it
  when adding or changing files.
- Use Cargo's standard `src/`, `tests/`, and `examples/` directories. Keep
  architecture-specific implementations in separate modules.
- Treat `socketry-` as the published package prefix. Use semantic module
  names within packages; the facade exposes `socketry::executor`.
- Keep the future executor in `crates/executor`, published as `socketry-executor`.
  Its public scheduler type is `Scheduler`, also exported as `socketry::Scheduler`.
- Create a new package when it has a useful implementation and a clear
  dependency boundary. Keep higher-level protocol packages in their own
  repositories when appropriate.

## Rust code

- Avoid abbreviations in source code. Prefer clear, consistent names over
  shortened names. Preserve names required by external APIs, traits, and
  vendored source.
- Follow Rust naming conventions and use the workspace's `cargo fmt` style.
- Keep public APIs small. Prefer private implementation details until a
  concrete consumer needs them.
- Use `Result` for expected failures. Document panic conditions and avoid
  `unwrap` and `expect` for recoverable failures in library code.
- Prefer standard types and established traits where their semantics fit.
  Document why a new abstraction is necessary.
- Use safe Rust by default. Keep unsafe code narrowly scoped, state its
  invariants in `SAFETY` comments, and document caller obligations on unsafe
  public functions.

## Runtime boundaries

- New concurrency work targets ordinary futures, without a coroutine stack
  per task. The coroutine implementation is saved on its own branch; see
  design.md for the migration plan.
- Protocol code depends on the I/O and task capabilities it uses. Keep
  concrete executors, OS selectors, and runtime adapters at explicit boundaries.
- Use `selector` for Socketry's native I/O backends. Keep platform modules
  under `scheduler/selector/` and runtime adapters such as Tokio alongside
  `scheduler/socketry.rs`. Upstream dependencies may use their own terminology.
- Select native implementations through target configuration and Cargo
  features. Share portable contracts and fallback helpers, retaining concrete
  future types. Readiness is a socket capability, not a regular-file fallback.
- Pass the task owner explicitly when a library starts child tasks. Scheduler
  and barrier ownership follow the same spawning contract. Contextual lookup
  may be a convenience at the application boundary.
- Document whether a future, handle, or resource is Send, Sync, or restricted
  to a thread. Let Rust check these properties. Do not add unsafe Send or Sync
  implementations solely to satisfy a spawning bound.
- Document cancellation at each asynchronous API: what happens before
  submission, during execution, after partial progress, and on drop.
- Destructors perform synchronous cleanup or request deferred cleanup.
  Methods that wait for shutdown return futures. Do not promise asynchronous
  cleanup has completed merely because a value was dropped.

## Performance and dependencies

- Make allocation, copying, locking, and reference counting costs explicit
  where they affect frequently used operations. Reuse registrations and
  buffers when their lifetimes permit it.
- Prefer concrete or generic futures. Introduce boxed futures and dynamic
  dispatch at deliberate boundaries rather than on every I/O operation.
- Keep optional runtime and platform dependencies out of portable packages.
  Adding a backend must not require every consumer to enable it.
- Distinguish a performance hypothesis from a measured result. A Rust port
  inherits an algorithm, not the upstream implementation's benchmark results.

## Documentation and verification

- Describe implemented behavior separately from planned behavior. Include
  ownership, shutdown, and runtime requirements in public API documentation.
- Keep agent-context.md current when implementation boundaries change. Put
  reusable Rust guidance in context/rust.md.
- Put public API tests in `tests/`; keep tests of private invariants beside
  the implementation or in a private test module.
- When tests or benchmarks are requested, use deterministic clocks and
  synchronization where possible. Record which platforms and configurations
  actually ran; distinguish compilation from execution.
- Retain upstream revision, paths, license, and attribution for ports. Keep
  verbatim vendor copies separate from translated Rust implementations.
