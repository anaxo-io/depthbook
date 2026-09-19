# Contributing

Thanks for taking the time to contribute.

## Development setup

```bash
git clone https://github.com/anaxo-io/depthbook
cd depthbook
cargo test --all-features
```

The toolchain is pinned in `rust-toolchain.toml`; `rustup` picks it up automatically.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
cargo doc --no-deps --all-features
```

All five must pass. CI runs the same commands plus an MSRV check against Rust 1.85 and `cargo deny check`.

## Guidelines

- **Tests are the contract.** `tests/property_tests.rs` encodes the book invariants (sortedness, best bid/ask, deduplication, zero-quantity removal). If a change requires weakening a property, explain why in the pull request.
- **No `unsafe`.** The crate is `#![forbid(unsafe_code)]`. The one exception is outside the crate: `benches/common/mod.rs` calls `sched_setaffinity` to pin the benchmark thread.
- **No new dependencies** without a reason that cannot be met by the standard library or an existing dependency.
- **Benchmarks** live in `benches/`. If a change targets performance, include before/after numbers from `cargo bench`, measured as described below.
- **Public items need docs.** The crate builds with `#![warn(missing_docs)]` and CI treats rustdoc warnings as errors.

## Benchmarks

**Publish numbers from an otherwise idle machine, and say so.** Every run prints
`unpinned` or `pinned to cores [2]` as its first line, so a pasted result always records
how it was measured.

`DEPTHBOOK_PIN` confines a run to named cores:

```bash
DEPTHBOOK_PIN=2 cargo bench --bench book_ops
```

Unset or empty means unpinned, which is the default and the right default. **Pinning is
not a reproducibility button, and on its own it is usually the wrong choice.**
`sched_setaffinity` constrains this process; it does not reserve the core, and any other
process may still be scheduled there. Measured on `book_ops`, three runs each way:

| | outliers | run-to-run spread of the median |
| --- | --- | --- |
| idle, unpinned | 7.0 % | 2.6 % |
| idle, pinned to core 2 | 7.3 % | 3.6 % |
| loaded, unpinned | 8.6 % | 2.3 % |
| loaded, pinned to core 2 | 8.8 % | 29.3 % |

Idle rows are the whole file, loaded rows are three of its benchmarks with twelve busy
cores alongside. Pinning bought nothing when idle and was far worse under load, because
the run then turns on whether something else lands on the chosen core. The scheduler finds
an idle core more reliably than a fixed guess.

Pinning is worth setting when the core is genuinely reserved, so that nothing competes for
it. That is how the README numbers were produced: a rented bare-metal box booted with

```
isolcpus=2-7 nohz_full=2-7 rcu_nocbs=2-7
```

and the `performance` governor, which leaves the operating system on core 0 and cores 2 to
7 idle, then `DEPTHBOOK_PIN=2` to land on one of them. In that setup the median confidence
interval across all 98 benchmarks was 0.20 % of the median, against 2.4 % mean on a shared
machine. Reproducing a published number means reproducing the boot line too, not just the
environment variable.

Check which logical CPUs share a physical core before choosing, since an SMT sibling is
not an independent core:

```bash
cat /sys/devices/system/cpu/cpu2/topology/thread_siblings_list   # e.g. "2-3"
```

The value is a comma-separated list, and threads a benchmark spawns inherit the mask.
`benches/store_ops.rs` has benchmarks that use up to eight threads, so pinning that file
to one core serialises them. Give it a core per thread, one per physical core:

```bash
DEPTHBOOK_PIN=2,4,6,8,10,12,14,0 cargo bench --bench store_ops
```

Pinning does not control CPU frequency, which on a `schedutil` governor is its own source
of drift. The README records the governor in force for the published numbers.

## Releasing

The procedure is the organisation's, written once in
[`anaxo-io/.github/RELEASING.md`](https://github.com/anaxo-io/.github/blob/main/RELEASING.md):
changelog written as changes land, a version chosen by a person, one release commit, and a
`vX.Y.Z` tag that triggers everything after. Read it before cutting a release.

What is specific to this crate:

- `cargo release X.Y.Z --execute` does the release commit, tag and push; `release.toml`
  holds the configuration and sets `publish = false`. Dry-run first by omitting
  `--execute`. Pull `main` before running it.
- Nothing goes to crates.io. The name `depthbook` is free but unclaimed, and the crate is
  consumed as a git dependency on a tag.
- Pre-1.0, a breaking change bumps the minor, and breaking includes what no tool can read
  off a commit subject: a raised MSRV, or a change to the JSON a `Book` serialises to,
  since that is a wire format for anything storing snapshots.
- A change to published benchmark numbers is not a release on its own, but the conditions
  they were measured under belong in the changelog entry when they change. See
  [Benchmarks](#benchmarks).
- `.github/workflows/release.yml` calls the shared `release-rust.yml`, which verifies the
  tag against `Cargo.toml`, runs `cargo package`, and creates the GitHub release from the
  matching `CHANGELOG.md` section.
- `v0.1.0` to `v0.6.0` were released by hand with `gh release create`, because a tag event
  uses the workflow from the tagged commit and none of those tags predates a working one.
  `v0.7.0` is the first automated release.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `perf:`, `refactor:`, `test:`, `chore:`.

## Licence

Contributions are dual-licensed under MIT and Apache-2.0, matching the crate.
