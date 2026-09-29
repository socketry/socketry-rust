# Conventions

- Use lowercase filenames for Markdown documents. In particular, use
  readme.md and license.md.
- Keep foundational Rust packages in this repository's Cargo workspace. Give
  published component packages the `socketry-` prefix.
- Keep the root Cargo package named `socketry`; use it as the convenient public
  facade for the foundational packages.
- Keep package APIs in separately publishable workspace members when they have
  a clear dependency and version boundary.
- Avoid abbreviations in source code. Prefer clear, consistent names over
  shortened names.
- Keep this file as the record of project-specific conventions and follow it
  when adding or changing files.
