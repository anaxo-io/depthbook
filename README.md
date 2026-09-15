# orderbook

[![CI](https://github.com/anaxo-io/orderbook-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/anaxo-io/orderbook-rs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://blog.rust-lang.org/)

In-process L2 order book state for market data: apply snapshots and deltas, keep the book
sorted, and find out when the feed dropped a message. L2 means aggregated by price level:
each level is a price and a total quantity, with no individual orders, order IDs or queue
positions. It is the shape most venues publish and the one strategies read.

## Why

Every exchange connector ends up rewriting the same three things: a sorted price ladder,
fixed-point price arithmetic, and the sequence bookkeeping that tells you when the book
is no longer trustworthy. The third one is where implementations quietly go wrong — it is
easy to apply an out-of-order delta and end up with a book that looks fine and is not.

This crate is those three things and nothing else. If you want a full trading framework
with venue connectors and an execution layer, [`barter-rs`](https://github.com/barter-rs/barter-rs)
is the better fit; this is a data structure you can drop into whatever you already have.

## Quick start

```toml
[dependencies]
orderbook = { git = "https://github.com/anaxo-io/orderbook-rs", tag = "v0.4.0" }
```

```rust
use orderbook::{f64_to_scale9, BookStore, Level};

fn main() -> Result<(), orderbook::Error> {
    let store = BookStore::new();

    // A snapshot establishes the book.
    store.apply_snapshot(
        "binance",
        "BTC-USDT",
        &[Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0))],
        &[Level::new(f64_to_scale9(50_010.0), f64_to_scale9(2.0))],
        1,
        1_700_000_000_000_000_000,
    )?;

    // Deltas move it forward. Zero quantity removes a price.
    store.apply_delta(
        "binance",
        "BTC-USDT",
        &[Level::new(f64_to_scale9(50_005.0), f64_to_scale9(0.5))],
        &[],
        2,
        1_700_000_000_000_000_001,
    )?;

    let (bid, ask) = store.bbo("binance", "BTC-USDT").unwrap();
    assert_eq!(bid.price, f64_to_scale9(50_005.0));
    assert_eq!(ask.price, f64_to_scale9(50_010.0));

    // A delta that skips a sequence number is refused, not applied. Recover by
    // applying a fresh snapshot.
    let skipped =
        store.apply_delta("binance", "BTC-USDT", &[], &[], 9, 1_700_000_000_000_000_002);
    assert!(skipped.is_err());
    assert_eq!(store.stats().sequence_gaps, 1);

    println!("best bid {} / best ask {}", bid.price, ask.price);
    Ok(())
}
```

## Features

- **Sorted fixed-capacity sides.** Each side is a `[Level; 200]` array kept in price order
  with the best level at the *end*, so an update near the top of the book, where nearly
  all updates land, moves almost nothing. Lookup scans the best 16 levels linearly and
  binary-searches the rest. No allocation on the update path. Invariants are
  property-tested in [`tests/property_tests.rs`](tests/property_tests.rs). The layout was
  chosen by measurement; see [Side layout](#side-layout) below.
- **Exact prices, in their own type.** Prices and quantities are `Scale9` — a
  `#[repr(transparent)]` wrapper over an `i64` holding the value times 10⁹. Being a
  distinct type rather than a bare integer means an unscaled number cannot be passed
  where a scaled one belongs, the mistake that silently misprices a book by a factor of
  a billion. `checked_add`, `checked_sub`, `checked_mul` and `checked_div` are
  overflow-checked and scale-aware; `Display` prints the decimal value.
- **Sequence-gap detection.** `apply_delta` compares the incoming sequence number with the
  book's, under the book's write lock. A gap returns `Error::SequenceGap` and leaves the
  book untouched: readers keep seeing the last good state, and the caller is expected to
  re-request a snapshot. A repeated sequence number is ignored. Counted in `stats()`.
- **Absolute quantities.** A delta level carries the new total quantity at that price, not
  a change to it; zero deletes the level. This is the aggregated L2 model most venues
  publish, so there are no order IDs and no queue positions.
- **Exchange timestamps preserved.** Both `apply_snapshot` and `apply_delta` record the
  timestamp you pass, not the local clock, so `is_stale` measures the venue's view of time.
- **Concurrent reads.** Books live in a `DashMap` keyed by venue and instrument, each behind
  an `RwLock`, with sequence and timestamp mirrored into atomics so gap and staleness checks
  never wait on a reader. Exercised in [`tests/concurrency_tests.rs`](tests/concurrency_tests.rs).
- **Interned identifiers.** Venue and instrument strings are deduplicated through a global
  cache, so a book copy does not copy the strings.

## Non-goals

- **No I/O and no exchange protocols.** Nothing here connects to a venue or parses an
  exchange message. Feeding the store is your job.
- **Not multi-process.** State lives in the process that owns the `BookStore`. There is no
  shared memory and no persistence; a restart starts empty.
- **Not lock-free.** Reads take a read lock, attempting a non-blocking acquire first and
  falling back to a blocking one under write contention.
- **Bounded depth.** A side holds at most 200 levels. Beyond that the worst level is dropped,
  which suits top-of-book work and does not suit full-depth archival.
- **One writer per instrument** is the intended pattern. Concurrent writers to the same book
  are safe, and each sequence number is applied at most once, but interleaved feeds will
  report gaps against each other.

## Documentation

```bash
cargo doc --open
```

Every public item is documented, and the examples in the docs are compiled and run as tests.
Two runnable examples live in [`examples/`](examples/):

```bash
cargo run --example orderbook_usage
cargo run --example store_usage
```

## Performance

```bash
cargo bench
```

Measured on an AMD Ryzen 7 1800X (8 cores / 16 threads) with Rust 1.98.1. The machine was
otherwise idle but CPU frequency scaling was left on, so expect a few percent of drift
between runs. Criterion medians; your numbers will differ. These come from [`benches/`](benches/)
and can be reproduced with the command above.

| Operation | Median |
| --- | --- |
| `apply_delta`, 1 level | 116 ns |
| `apply_delta`, 5 levels | 184 ns |
| `apply_delta`, 20 levels | 778 ns |
| `apply_snapshot`, 10 levels per side | 927 ns |
| `apply_snapshot`, 100 levels per side | 2.21 µs |
| `apply_snapshot`, 200 levels per side | 3.76 µs |
| `bbo` | 65 ns |
| `snapshot`, any depth | ~470 ns |
| `Side::best` | 1.4 ns |
| `f64_to_scale9` | 3.1 ns |
| 100,000 sequential deltas | 13.5 ms (≈7.4M deltas/sec) |

Three things worth reading off that table:

- **`apply_delta` is about 35 ns per level on top of a fixed 80 ns.** The fixed part is
  the map probe, the write lock and the sequence check; the per-level part is the scan
  and the in-place update. The benchmark updates levels 5 to 24 from the top, so most of
  them fall past the 16-level linear scan into the binary-search path; updates at the very
  top are cheaper. An earlier version of this table showed the call as flat in the number
  of levels, which was a benchmark bug: it resubmitted the same sequence number and
  measured the duplicate short-circuit.
- **`snapshot` is flat in `depth`, and that is a wart, not a feature.** Asking for 5 levels
  costs the same ~470 ns as asking for 100, because the book is cloned in full and then
  truncated. If you only need top of book, `bbo` is about 7× cheaper. This is tracked in
  [#2](https://github.com/anaxo-io/orderbook-rs/issues/2).
- **`Scale9` costs nothing.** Wrapping prices in a distinct type rather than using a bare
  `i64` left every benchmark within noise of the untyped version, which is what
  `#[repr(transparent)]` and inlined accessors should give you.

Concurrency benchmarks measure whole batches rather than single calls: `concurrent_reads/8`
runs 8 threads doing 1,000 reads each in 1.75 ms total, and `write_with_concurrent_reads`
performs 100 writes against 4 live reader threads in 286 µs.

### Side layout

David Gross's CppCon 2024 talk *When Nanoseconds Matter* makes two claims about
price-level arrays: store them with the best price at the end so top-of-book inserts move
few elements, and search them linearly because a short sequential scan beats a binary
search on the levels that actually get touched.
[`benches/side_layout.rs`](benches/side_layout.rs) tests both, plus two standard-library
baselines, against this crate's original layout on a 1,000-update stream where the
distance from the best level is geometric, a fifth of updates are deletes, and deleted
levels get re-added. Same machine as above.

| Layout | stream, 50 levels | stream, 200 levels | update best, 200 | add+remove at top, 200 | add+remove mid, 200 |
| --- | --- | --- | --- | --- | --- |
| `BTreeMap<Scale9, Scale9>` | 27.5 µs | 31.4 µs | 14.1 ns | 49.0 ns | 52.5 ns |
| `Vec<Level>`, best-first, binary search | 24.8 µs | 46.1 µs | 14.0 ns | 166.4 ns | 114.4 ns |
| fixed array, best-first, binary search (v0.3.0) | 23.9 µs | 41.4 µs | 12.6 ns | 163.8 ns | 112.1 ns |
| fixed array, best-last, binary search | 19.1 µs | 19.1 µs | 12.6 ns | 31.8 ns | 111.5 ns |
| fixed array, best-first, linear scan | 11.0 µs | 26.8 µs | 2.9 ns | 120.6 ns | 166.3 ns |
| fixed array, best-last, linear scan | 8.4 µs | 8.4 µs | 4.6 ns | 12.1 ns | 194.4 ns |
| fixed array, best-last, scan 16 then binary (v0.4.0) | 10.2 µs | 10.3 µs | 4.1 ns | 13.3 ns | 125.6 ns |

Two standard-library baselines are included because "why not a `BTreeMap`?" is the
question a fixed array has to answer. It does, but only after the layout change: the
original best-first array is no better than a plain `Vec` and loses to `BTreeMap` at 200
levels, because every top-of-book insert shifts the whole array. Reversing the array
makes the side flat in depth, the linear scan makes the common case about three times
cheaper, and pure linear pays for it on deep inserts. `BTreeMap` remains the best
structure for inserts deep in the book, at about half the cost, which is the trade being
made here: a book that updates at the top constantly and in the middle rarely. The hybrid
keeps the top-of-book win and bounds the deep case, so that is what `Side` now uses.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports with a failing test are especially
welcome.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed
as above, without any additional terms or conditions.
