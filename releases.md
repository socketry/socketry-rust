# Releases

## v0.2.0

- Rename the positioned file I/O trait from `FileIO` to `FileIo` in `socketry` and `socketry-executor`, including the public `scheduler` module. Update imports and trait bounds; the old spelling is removed without a compatibility alias.

## v0.1.5

- Cover io\_uring initialization failures, readiness retries, cancellation, and shutdown while making request ownership invariants explicit.
- Expose the conventional `FileIO` name with `FileIo` compatibility aliases, extract portable scheduler contracts, and require coverage for every supported platform and feature implementation.

## v0.1.4

- Adopt `socketry-project` 0.3.7 for shared project tasks and Markdown normalization.
- Require the aggregate test and coverage result for pull request merges.

## v0.1.3

- Use the shared Socketry Project tasks and update agent context setup guidance.

## v0.1.2

- Use the shared `socketry-project` Releasing skill for the standard release process and remove references to the duplicate Bake Cargo publishing context.

## v0.1.1

- Create or update GitHub Releases after successful crates.io publication.
- Move implementation and design guidance into the package's public context.
- Add Bake Agent Context tasks to the repository's development workspace.
