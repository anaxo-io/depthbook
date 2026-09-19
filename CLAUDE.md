# CLAUDE.md

Guidance for Claude Code working in this repository.

## What this is

`depthbook` is in-process L2 order book state: apply snapshots and deltas, keep each side
sorted, and detect when the feed dropped a message. L2 means aggregated by price level, so
there are no order IDs, no queue positions and no matching. A delta quantity replaces the
total at that price; zero deletes the level.

The crate does no I/O and speaks no exchange protocol. Feeding it is the caller's job, and
requests to add a venue connector belong in a different repository.

## Quality gates

Everything below must pass before a change is done. CI runs all of it plus `cargo deny`.

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
cargo doc --no-deps --all-features
```

## What this crate is careful about

`CONTRIBUTING.md` has the rules in full. The short version: the crate is
`#![forbid(unsafe_code)]`, and the only `unsafe` anywhere is the `sched_setaffinity` call
in `benches/common/mod.rs`. `tests/property_tests.rs` encodes the side invariants and is
the contract. A sequence gap leaves the last good state readable and sets a `gapped` flag
rather than blocking reads; a snapshot older than the book is refused with
`Error::OutOfOrder`. Both are deliberate and documented; do not redesign them without
asking.

## Benchmarks

Two standing rules:

- Every number in the README must be reproducible by a reader with a command that is
  written down, including `DEPTHBOOK_PIN`. No unsourced figures.
- State the machine and its isolation next to the numbers. The published set comes from a
  box booted with `isolcpus`, not a laptop.

Pinning is not a reproducibility button: on a machine whose cores are not reserved it
measures the scheduler. `CONTRIBUTING.md` has the measurements that show this.

## Repository conventions

- Dual licensed MIT OR Apache-2.0.
- `CHANGELOG.md` follows Keep a Changelog. **Dependency bumps get a changelog entry too**,
  saying what the upgrade needed, not just the version pair.
- Conventional-commit subjects. Do not commit unless asked.
- Releases go out by pushing a `vX.Y.Z` tag. The procedure is the organisation's, in
  https://github.com/anaxo-io/.github/blob/main/RELEASING.md; `CONTRIBUTING.md` has
  what is specific to this crate. Never re-copy the workflow body here — `release.yml`
  calls the shared one.
- Nothing is published to crates.io. The name `depthbook` is free but unclaimed.
- `gh pr checks` returns nothing for this repo. Read status from
  `gh run list --json conclusion` and `gh run view <id> --json jobs` instead.
