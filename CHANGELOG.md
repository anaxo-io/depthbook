# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-14

Initial release.

### Added

- `Side`: one side of a book as a fixed-capacity (200 level) sorted array with binary-search insert.
- `Book`: a venue/instrument book with bid and ask sides, timestamp, and sequence number.
- `BookStore`: concurrent store keyed by venue and instrument, with snapshot and delta application, best bid/offer queries, staleness checks, and sequence-gap detection.
- `Scale9`: 9-decimal fixed-point integer representation with checked arithmetic (`scale9_add`, `scale9_sub`, `scale9_mul`, `scale9_div`) and conversion helpers.
- `InternedString`: deduplicated venue and instrument identifiers.
- Property tests covering book invariants, concurrency tests covering reader/writer contention, and Criterion benchmarks for book and store operations.

### Known issues

- `Book::mid_price` and `Book::spread` do not check for a crossed book (best bid above best ask); a crossed book yields a negative spread rather than an error. Tracked in [#1](https://github.com/anaxo-io/orderbook-rs/issues/1).
- `BookStore::snapshot` clones the full book before truncating to the requested depth, so a shallow read costs the same as a deep one. Tracked in [#2](https://github.com/anaxo-io/orderbook-rs/issues/2).
- `Side::insert` silently drops the worst level when a side is at capacity. This is intentional for top-of-book use but is not reported to the caller. Tracked in [#3](https://github.com/anaxo-io/orderbook-rs/issues/3).

[Unreleased]: https://github.com/anaxo-io/orderbook-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/anaxo-io/orderbook-rs/releases/tag/v0.1.0
