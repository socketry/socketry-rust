use bake::{Registry, Result};
use bake_agent_context as _;

fn main() -> Result<()> {
    Registry::discover()?.run()
}
