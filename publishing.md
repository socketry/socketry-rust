# Publishing

This workspace publishes each Cargo package separately. The GitHub Actions
workflow uses crates.io Trusted Publishing for subsequent releases.

For each crate's first release, publish manually with an owner token, then
configure its Trusted Publisher on crates.io with:

- GitHub owner: `socketry`
- Repository: `socketry-rust`
- Workflow: `publish.yml`
- Environment: `crates-io`

Tag a component release as `socketry-concurrent-vVERSION` and the facade release
as `socketry-vVERSION`. Publish `socketry-concurrent` before `socketry`, since
the facade depends on it.
