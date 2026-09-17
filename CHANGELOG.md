# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Benchmark core pinning. `DEPTHBOOK_PIN=2 cargo bench` confines a run to named cores via
  `sched_setaffinity`; unset or empty means unpinned, which stays the default. Every run
  prints `pinned to cores [2]` or `unpinned` as its first line, so a pasted result records
  how it was measured. The helper is `benches/common/mod.rs`, and the three bench binaries
  now have an explicit `main` because `criterion_main!` leaves no hook before it. `libc`
  is a new dev-dependency, already present in the lock file as a transitive dependency of
  criterion and proptest.

### Changed

- **All published benchmark numbers re-measured** on a Scaleway Elastic Metal EM-A116X
  (Intel Xeon E3-1231 v3) booted with `isolcpus=2-7 nohz_full=2-7 rcu_nocbs=2-7` and the
  `performance` governor, pinned to one isolated core. The README now states those
  conditions and the exact command next to every table.

  What pinning changed: on its own, nothing worth having. On a shared machine, pinning
  without core isolation left the outlier rate unchanged (7.0 % against 7.3 %) and made
  run-to-run medians *less* stable (2.6 % spread against 3.6 %), because affinity confines
  the benchmark without reserving the core. Combined with `isolcpus`, the median confidence
  interval across 98 benchmarks is 0.20 % of the median, against 2.4 % mean on the previous
  shared machine. The isolation is what bought the stability; pinning is what makes the
  isolation usable. The two figures come from different machines, so that is not a clean
  before-and-after for pinning alone.

  Three previously published claims are contradicted by the new data and have been
  corrected rather than silently replaced:

  - `apply_delta` was described as costing a flat 19 ns per level above a fixed 110 ns. The
    per-level cost in fact grows with depth, from about 18 ns for the first few levels to
    52 ns by level 20, as deeper levels fall past the 16-level linear scan.
  - `snapshot` was described as flat in `depth` at about 470 ns, and `bbo` as 7× cheaper.
    It is 338 ns at depth 5 rising 8 % to 364 ns at depth 100, and `bbo` is about 5×
    cheaper.
  - The concurrency figures (`concurrent_reads/8` at 1.75 ms, `write_with_concurrent_reads`
    at 371 µs) have been **withdrawn, not updated**. Threads inherit the parent's affinity,
    so the pinned run serialised every reader onto one core; the times scale linearly with
    thread count, which is that serialisation rather than contention. Measuring them needs
    a core per thread and has not been done.

  The side-layout comparison keeps its ordering on every measure, and the conclusion that
  `Side` should use the best-last hybrid is unchanged.

## [0.6.0] - 2026-09-16

### Changed

- **Breaking.** The crate is renamed from `orderbook` to `depthbook`, because the
  crates.io name `orderbook` belongs to an unrelated project. Update imports from
  `use orderbook::..` to `use depthbook::..`. The GitHub repository moved from
  `anaxo-io/orderbook-rs` to `anaxo-io/depthbook`; GitHub redirects the old URL.

## [0.5.0] - 2026-09-15

### Added

- `BookStore::is_gapped` and a `gapped` field on `Book`: set when a delta is rejected for
  a sequence gap, cleared by the next snapshot. The book keeps serving its last good
  state; this is how a reader finds out that state is behind the venue.
- `BookStore::remove`, for delisted instruments and for venues that restart their
  sequence numbering after a reconnect.
- `Stats::snapshots_rejected`.

### Changed

- **Breaking.** `apply_snapshot` rejects a snapshot whose sequence number is below the
  book's with the new `Error::OutOfOrder`, checked under the write lock, so a stale
  snapshot can no longer roll back a concurrently applied delta. `BookState::replace`
  returns `Result` accordingly.
- **Breaking.** `apply_snapshot` and `apply_delta` reject negative prices and quantities
  with `Error::InvalidData`, matching what deserialisation already enforced.
- Deserialising a `Book` checks that bids descend and asks ascend; a `Side` on its own
  still accepts either direction because it does not know which it is.

### Fixed

- `str_to_scale9` accepts the exact lower bound `-9223372036.854775808`.
- `depth_within_bps` saturates instead of truncating when the margin overflows `i64`.
- Benchmarks: `apply_delta/N` is now labelled as N levels per side, the multi-instrument
  benchmark advances a sequence counter per instrument instead of resubmitting the same
  one, the concurrent-write benchmark starts its readers behind a barrier, and the layout
  stream replays against a fresh copy each sample.

## [0.4.0] - 2026-09-15

### Changed

- **Breaking.** `Side::levels` returns a best-first iterator instead of a slice. Levels
  are now stored best-last so a top-of-book insert moves almost nothing, and lookup scans
  the best 16 levels before binary-searching the rest. On a top-heavy update stream at
  200 levels the side is about four times faster; an insert in the middle of a 200-level
  side is about 12% slower (112 ns to 126 ns).
  Measured in `benches/side_layout.rs` and written up in the README.
- `BookStore` looks up books without allocating: the two `String` copies of venue and
  instrument on every `apply_delta`, `bbo`, `snapshot` and `is_stale` call are gone.

## [0.3.0] - 2026-09-15

### Fixed

- The sequence check in `apply_delta` now runs under the book's write lock, so two writers
  can no longer both apply the same sequence number. `BookState::apply` returns
  `Result<bool>` accordingly.
- `str_to_scale9` parses exactly instead of through `f64`: all nine decimals survive,
  and `NaN`, `inf`, exponents and over-long fractions are rejected. **Breaking:** it now
  returns `Error::InvalidData` rather than `ParseFloatError`.
- `scale9_to_string` keeps the sign of values between -1 and 0 and carries rounding
  (`0.999` at two decimals is `1.00`, not `0.100`).
- Deserialising a `Side` rejects unsorted or duplicate prices, non-positive quantities,
  and a `count` that does not match `levels`.
- `mid_price`, `spread` and `depth_within_bps` no longer overflow on extreme prices.
- The `apply_delta`, sustained-throughput and concurrent-write benchmarks reused one
  sequence number, so they measured the duplicate short-circuit rather than an update.
  README figures are re-measured.

## [0.2.0] - 2026-09-14

### Changed

- **Breaking.** Prices and quantities are now a distinct `Scale9` type rather than a bare
  `i64`. `Level::new`, `Side::insert`, `Side::remove`, `Side::update`,
  `Side::depth_within_bps`, `Book::mid_price` and `Book::spread` all take or return
  `Scale9`. This makes it a compile error to pass an unscaled number where a scaled one is
  expected — previously `Level::new(50_000, 1)` compiled and silently meant `0.00005`.

  Migrating: wrap raw scaled integers with `Scale9::from_raw(..)`, or build values with
  `f64_to_scale9(..)` / `str_to_scale9(..)`. `.raw()` recovers the underlying `i64`. Code
  already using `f64_to_scale9` needs no change.

- **Breaking.** The free functions `scale9_add`, `scale9_sub`, `scale9_mul` and
  `scale9_div` are replaced by inherent methods `Scale9::checked_add`, `checked_sub`,
  `checked_mul` and `checked_div`. `Scale9` also implements `Add`, `Sub`, `Neg`, `Ord`
  and `Display`.

- `Side`'s capacity is exposed as the `MAX_LEVELS` constant instead of a bare `200`.

- Update `criterion` from 0.5 to 0.8. Benchmarks now use `std::hint::black_box`;
  `criterion::black_box` is deprecated in 0.8 and the crate builds benchmarks with
  warnings denied ([#6](https://github.com/anaxo-io/depthbook/pull/6)).

- Update `serial_test` from 3.1 to 4.0
  ([#7](https://github.com/anaxo-io/depthbook/pull/7)).

- Update `actions/checkout` from 4 to 7 in CI
  ([#5](https://github.com/anaxo-io/depthbook/pull/5)).

### Added

- `Scale9::ZERO`, `Scale9::ONE`, `Scale9::from_raw`, `Scale9::raw`, `Scale9::from_f64`,
  `Scale9::to_f64`, `Scale9::is_zero` and `Scale9::saturating_add`. `Debug` and `Display`
  print the decimal value rather than the raw integer.

### Fixed

- Stop Dependabot proposing bumps to the `dtolnay/rust-toolchain` reference in the MSRV
  job. That tag names the Rust toolchain version rather than the action version, so a bump
  asked CI to install a Rust release that does not exist. The reference now changes only
  when the MSRV itself changes, alongside `rust-version` in `Cargo.toml`
  ([#4](https://github.com/anaxo-io/depthbook/pull/4)).

## [0.1.0] - 2026-09-14

Initial release.

### Added

- `Side`: one side of a book as a fixed-capacity (200 level) sorted array with
  binary-search insert.
- `Book`: a venue/instrument book with bid and ask sides, timestamp, and sequence number.
- `BookStore`: concurrent store keyed by venue and instrument, with snapshot and delta
  application, best bid/offer queries, staleness checks, and sequence-gap detection.
- `Scale9`: 9-decimal fixed-point representation with checked arithmetic and conversion
  helpers.
- `InternedString`: deduplicated venue and instrument identifiers.
- Property tests covering book invariants, concurrency tests covering reader/writer
  contention, and Criterion benchmarks for book and store operations.

### Known issues

- `Book::mid_price` and `Book::spread` do not check for a crossed book (best bid above
  best ask); a crossed book yields a negative spread rather than an error. Tracked in
  [#1](https://github.com/anaxo-io/depthbook/issues/1).
- `BookStore::snapshot` clones the full book before truncating to the requested depth, so
  a shallow read costs the same as a deep one. Tracked in
  [#2](https://github.com/anaxo-io/depthbook/issues/2).
- `Side::insert` silently drops the worst level when a side is at capacity. This is
  intentional for top-of-book use but is not reported to the caller. Tracked in
  [#3](https://github.com/anaxo-io/depthbook/issues/3).

[Unreleased]: https://github.com/anaxo-io/depthbook/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/anaxo-io/depthbook/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/anaxo-io/depthbook/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/anaxo-io/depthbook/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/anaxo-io/depthbook/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/anaxo-io/depthbook/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/anaxo-io/depthbook/releases/tag/v0.1.0
