// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::{Registry, Result};

fn run(registry: Result<Registry>) -> Result<()> {
    registry?.run()
}

fn main() -> Result<()> {
    run(Registry::discover())
}

#[path = "bake_generated_tasks/mod.rs"]
mod bake_generated_tasks;

#[cfg(test)]
mod tests {
    use super::run;
    use bake::Error;

    #[test]
    fn registry_discovery_errors_are_returned() {
        let result = run(Err(Error::new("injected discovery failure")));

        assert_eq!(
            result.unwrap_err().to_string(),
            "injected discovery failure"
        );
    }
}
