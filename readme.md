# socketry

Foundational concurrency APIs for Rust projects in the Socketry ecosystem.

This package is the convenient entry point. It re-exports the focused
`socketry-concurrent` package, which contains the stackful fiber and cooperative
scheduler implementation.

## Usage

```toml
[dependencies]
socketry = "0.1"
```

```rust
use socketry::Scheduler;

fn main() -> std::io::Result<()> {
	let mut scheduler = Scheduler::new(256 * 1024);
	scheduler.spawn(async {
		// Await ordinary Rust futures here.
	})?;
	scheduler.run();
	Ok(())
}
```

The detailed implementation notes and platform support are documented in the
[`socketry-concurrent` package](https://github.com/socketry/socketry-rust/tree/main/crates/concurrent).
