# socketry

Foundational concurrency APIs for Rust projects in the Socketry ecosystem.

The `socketry` package re-exports `socketry-executor`: owned asynchronous
tasks, explicit child barriers, and a futures executor with multiple workers
and work stealing. Tasks use ordinary future polling and have no private
coroutine stacks.

## Usage

```toml
[dependencies]
socketry = "0.1"
```

```rust
use socketry::{Scheduler, yield_now};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scheduler = Scheduler::with_workers(4)?;
    let task = scheduler.spawn(async {
        yield_now().await;
        42
    })?;

    let answer = scheduler.block_on(task)?;
    assert_eq!(answer, 42);
    Ok(())
}
```

Workers start when the scheduler is constructed. `spawn` accepts
`Send + 'static` futures and returns an awaitable result handle. Dropping that
handle leaves the task owned by its scheduler or barrier. Dropping the
scheduler requests cancellation and joins workers when called outside a worker.

Use `scheduler.barrier()` to own children explicitly. Both schedulers and
barriers implement `Spawn`. `barrier.wait().await` waits for completion;
`barrier.stop().await` closes the barrier, cancels children, and waits for their
synchronous destructors. Dropping a barrier requests cancellation without
waiting. Observe individual results and panics through task handles.

`Scheduler::current()` and `Task::current()` provide contextual lookup while
tasks run. Use `.await` to suspend; synchronous blocking calls occupy a worker.

See the [executor package](crates/executor/readme.md) for scheduling,
ownership, cancellation and allocation details. An executable example is:

```sh
cargo run --package socketry-executor --example work_stealing
```

## Portable I/O and runtime selection

Generic code can accept `Network`, `FileIo`, `Clock`, and `Spawn` capabilities.
Socketry and the optional Tokio adapter implement these contracts with concrete
future and resource types. Import the traits to call their methods.

| Cargo configuration | Implementation |
| --- | --- |
| Default (`native`) | Socketry tasks; epoll, kqueue or Windows IOCP/AFD socket readiness through async-io. |
| `features = ["io-uring"]` on Linux | Native completion reads/writes through an io_uring selector. |
| `features = ["tokio"]` | Also expose `socketry::scheduler::tokio::Scheduler`, adapting an existing Tokio runtime. |
| `default-features = false, features = ["tokio"]` | Tokio adapter without Socketry's native I/O dependencies. |
| `default-features = false` | Task executor and portable contracts, without I/O implementations. |

Socket registrations persist across operations and worker migration. Reads and
writes take a reusable owned `Vec<u8>` and return `(io::Result<usize>, Vec<u8>)`.
Reads fill the buffer's existing length; allocate it with `vec![0; capacity]`.
Operations may transfer fewer bytes than requested. Dropping a future can
abandon an operation that has already consumed or transmitted bytes.

Run the same TCP exchange with either runtime:

```sh
cargo run -p socketry-executor --example portable_io
cargo run -p socketry-executor --example portable_io --no-default-features --features tokio
# Linux native completion:
cargo run -p socketry-executor --example portable_io --features io-uring
```

The Tokio adapter needs a live runtime with I/O and time enabled. It preserves
explicit task/barrier ownership, and its asynchronous `shutdown().await` joins
task destruction. Passing its handle explicitly selects Tokio; Socketry's
`Scheduler::current()` continues to identify Socketry execution.

## Current scope

TCP connect/accept/read/write/readiness, positioned file reads/writes, and sleep
are implemented. Regular files use blocking pools except for Linux io_uring.
Windows socket readiness uses IOCP/AFD; native overlapped file operations are
not implemented. Socketry currently uses async-io's shared readiness reactor
and timers; the io-event timer port remains planned in the
[design guide](context/design.md).

The io_uring selector owns a dedicated thread, retains buffers until terminal
completions, and drains cancellation during shutdown. Connection setup and
readiness waits still use async-io. Kernel support is probed when the selector
is first needed; failures are returned without silently falling back. Operation
pooling, registered buffers, UDP, arbitrary descriptor APIs, and a local
`!Send` task executor remain future work.

The former coroutine implementation is preserved on branch `coroutine`, at
commit `b520f3d`. Its native sources, stack allocation, nested synchronous
`wait` and task transfer are absent from the future executor.

## Agent Context

This crate includes an [implementation guide](context/implementation.md) and a
[design guide](context/design.md). Shared Rust guidance is provided by the
`bake-agent-context` task dependency. Install it into the repository with:

```sh
cargo bake agent:context:install
```

Install the launcher once with `cargo install socketry-cargo-bake --locked`.
The generated `.agents/context/` directory is ignored by Git. Repository-only
conventions and publishing instructions live under `.agents/` in the source
checkout.
