# depthbook

[![CI](https://github.com/anaxo-io/depthbook/actions/workflows/ci.yml/badge.svg)](https://github.com/anaxo-io/depthbook/actions/workflows/ci.yml)
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
depthbook = { git = "https://github.com/anaxo-io/depthbook", tag = "v0.6.0" }
```

```rust
use depthbook::{f64_to_scale9, BookStore, Level};

fn main() -> Result<(), depthbook::Error> {
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
  book's, under the book's write lock. A gap returns `Error::SequenceGap`, leaves the
  levels untouched and flags the book as gapped: readers keep seeing the last good state,
  and can tell it is behind the venue through `is_gapped()` or the `gapped` field on a
  snapshot. The next snapshot clears the flag. A repeated sequence number is ignored, and a
  snapshot older than the book is rejected with `Error::OutOfOrder`; call `remove()` first
  if the venue has restarted its numbering. All of it is counted in `stats()`.
- **Absolute quantities.** A delta level carries the new total quantity at that price, not
  a change to it; zero deletes the level. This is the aggregated L2 model most venues
  publish, so there are no order IDs and no queue positions.
- **Exchange timestamps preserved.** Both `apply_snapshot` and `apply_delta` record the
  timestamp you pass, not the local clock. `is_stale` compares that venue timestamp with
  the local wall clock, so a feed that stops sending goes stale even if the connection
  stays up.
- **Concurrent reads.** Books live in a `DashMap` keyed by venue and instrument, each behind
  an `RwLock`. Sequence, timestamp and the gapped flag are mirrored into atomics, so
  `is_stale` and `is_gapped` never take the lock. Writes do, which is what makes the
  sequence check race-free. Exercised in
  [`tests/concurrency_tests.rs`](tests/concurrency_tests.rs).
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
DEPTHBOOK_PIN=2 cargo bench --bench book_ops
DEPTHBOOK_PIN=2 cargo bench --bench store_ops
```

Measured on a Scaleway Elastic Metal EM-A116X: Intel Xeon E3-1231 v3 (Haswell, 4 cores /
8 threads, 8 MiB L3 shared by the whole package), Ubuntu 26.04 LTS, kernel
7.0.0-15-generic, rustc 1.98.1. The box rents by the hour for about EUR 0.08, so these are
reproducible for the price of a coffee.

The machine was booted with `isolcpus=2-7 nohz_full=2-7 rcu_nocbs=2-7` and the
`performance` governor, which leaves the operating system on core 0 and cores 2 to 7 with
nothing scheduled on them. `DEPTHBOOK_PIN=2` then puts the benchmark on one of those
isolated cores alone. That combination is what makes the numbers stable; pinning on a
shared machine does not, as [`CONTRIBUTING.md`](CONTRIBUTING.md#benchmarks) shows.
Criterion defaults, 100 samples per benchmark. Across all 98 benchmarks the median
confidence interval was 0.20 % of the median and the worst was 6.3 %; criterion's outlier
rate had a median of 1 % and a maximum of 31 %, the high rates being the sub-nanosecond
benchmarks where timer quantisation alone marks samples as outlying.

| Operation | Median |
| --- | --- |
| `apply_delta`, 1 level per side | 122 ns |
| `apply_delta`, 5 levels per side | 192 ns |
| `apply_delta`, 20 levels per side | 834 ns |
| `apply_snapshot`, 10 levels per side | 867 ns |
| `apply_snapshot`, 100 levels per side | 2.34 µs |
| `apply_snapshot`, 200 levels per side | 4.02 µs |
| `bbo` | 63 ns |
| `snapshot`, depth 5 | 338 ns |
| `snapshot`, depth 100 | 364 ns |
| `Side::best` | 1.3 ns |
| `f64_to_scale9` | 3.9 ns |
| 100,000 sequential deltas | 13.2 ms (≈7.6M deltas/sec) |

Three things worth reading off that table:

- **`apply_delta` costs about 105 ns fixed, and the per-level cost grows with depth.** The
  fixed part is the map probe, input validation, the write lock and the sequence check.
  The per-level part is roughly 18 ns for the first few levels, 25 ns by level 10 and 52 ns
  by level 20, because each row updates levels 5 to 24 from the top on *both* sides: the
  shallow ones are served by the 16-level linear scan, the deeper ones fall into the
  binary-search path and shift more of the array. An earlier version of this section quoted
  a flat 19 ns per level, which only ever held over the first few.
- **`snapshot` is nearly flat in `depth`, and that is a wart, not a feature.** Asking for 5
  levels costs 338 ns against 364 ns for 100, because the book is cloned in full and then
  truncated; the 8 % difference is the truncation, not the copy. If you only need top of
  book, `bbo` is about 5× cheaper. This is tracked in
  [#2](https://github.com/anaxo-io/depthbook/issues/2).
- **`Scale9` costs nothing.** Wrapping prices in a distinct type rather than using a bare
  `i64` left every benchmark within noise of the untyped version, which is what
  `#[repr(transparent)]` and inlined accessors should give you.

**No concurrency numbers are published here.** `concurrent_reads` and
`write_with_concurrent_reads` exist in [`benches/store_ops.rs`](benches/store_ops.rs), but
threads inherit the parent's CPU affinity, so a run pinned to one isolated core serialises
every reader onto that core. The measured times scale almost exactly linearly with thread
count, which is the signature of that serialisation rather than of contention. Measuring
them properly needs a core per thread, `DEPTHBOOK_PIN=2,3,4,5,6,7` on this machine, and
they have not been re-measured that way.

### Side layout

```bash
DEPTHBOOK_PIN=2 cargo bench --bench side_layout
```

David Gross's CppCon 2024 talk *When Nanoseconds Matter* makes two claims about
price-level arrays: store them with the best price at the end so top-of-book inserts move
few elements, and search them linearly because a short sequential scan beats a binary
search on the levels that actually get touched.
[`benches/side_layout.rs`](benches/side_layout.rs) tests both, plus two standard-library
baselines, against this crate's original layout on a 1,000-update stream where the
distance from the best level is geometric, a fifth of updates are deletes, and deleted
levels get re-added. Same machine and command conditions as above.

| Layout | stream, 50 levels | stream, 200 levels | update best, 200 | add+remove at top, 200 | add+remove mid, 200 |
| --- | --- | --- | --- | --- | --- |
| `BTreeMap<Scale9, Scale9>` | 22.0 µs | 25.4 µs | 14.3 ns | 45.1 ns | 41.8 ns |
| `Vec<Level>`, best-first, binary search | 21.1 µs | 33.9 µs | 16.2 ns | 126 ns | 104 ns |
| fixed array, best-first, binary search (v0.3.0) | 20.2 µs | 31.8 µs | 15.6 ns | 124 ns | 91.2 ns |
| fixed array, best-last, binary search | 14.7 µs | 19.1 µs | 14.8 ns | 46.2 ns | 91.9 ns |
| fixed array, best-first, linear scan | 8.39 µs | 17.3 µs | 2.90 ns | 82.3 ns | 128 ns |
| fixed array, best-last, linear scan | 5.36 µs | 5.33 µs | 3.93 ns | 12.1 ns | 136 ns |
| fixed array, best-last, scan 16 then binary (v0.4.0) | 7.33 µs | 7.36 µs | 4.75 ns | 14.0 ns | 100 ns |

There is no single winner, and which layout is best depends entirely on where in the book
the updates land.

**Scanning from the best end is O(1) for top-of-book work; binary search is O(log N).**
Going from 50 to 200 levels, the three scanning layouts do not move on `update best`:
2.90 to 2.90 ns, 3.91 to 3.93 ns, and 4.75 to 4.75 ns. The four binary-search layouts all
grow: `BTreeMap` from 11.1 to 14.3 ns and best-last binary search from 11.6 to 14.8 ns.
That is the talk's second claim, confirmed.

**Best-last is what makes the stream flat in depth.** Best-last linear scan holds at
5.36 and 5.33 µs across the fourfold depth increase, and the shipped hybrid at 7.33 and
7.36 µs, while best-first linear scan doubles from 8.39 to 17.3 µs because a top-of-book
insert still shifts everything below it. That is the talk's first claim, confirmed, and it
is worth more than the scan on this workload.

**`BTreeMap` wins deep in the book, by a lot.** At 41.8 ns for an insert and removal in
the middle of a 200-level side it beats every array layout, and beats the shipped hybrid
by a factor of 2.4. An array has to move memory where a tree relinks pointers.

So the trade is: the array layouts buy top-of-book speed, roughly 3× on the stream and 3 to
4× on an insert at the top, and pay for it in the middle of the book. That suits a feed
whose updates cluster near the touch, which is what the geometric distance in this
benchmark models and what venue L2 feeds actually look like. A workload that rewrites deep
levels uniformly should use a `BTreeMap` instead, and this crate would be the wrong choice
for it. Between the two best array layouts, pure best-last linear scan is fastest on every
top-of-book measure but worst of all seven in the middle at 136 ns; the shipped hybrid
gives up 38 % on the stream to cut the middle case to 100 ns. That bound is why `Side`
uses the hybrid.

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
