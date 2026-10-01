// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::{Context, Registry, Result};
use bake_agent_context as _;
use bake_cargo as _;
use bake_license as _;
use bake_readme as _;
use bake_releases as _;

#[bake::task(name = "cargo:after_version_bump")]
fn after_version_bump(context: &mut Context, version: String) -> Result<()> {
    context.call("license:update", &[])?;
    context.call("releases:update", &[&format!("v{version}")])?;
    context.call("readme:update", &[])?;
    Ok(())
}

fn main() -> Result<()> {
    Registry::discover()?.run()
}
