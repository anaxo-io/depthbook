//! Comprehensive benchmarks for BookStore operations
//!
//! These benchmarks validate that the BookStore meets performance targets:
//! - Snapshot retrieval: <1μs (p99)
//! - Sustained throughput: 100k updates/s per instrument
//! - Store capacity: 1000 instruments without degradation

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use orderbook::store::BookStore;
use orderbook::types::Level;
use orderbook::Scale9;
use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Helper function to generate test levels
fn generate_levels(count: usize, base_price: i64, is_bid: bool) -> Vec<Level> {
    (0..count)
        .map(|i| {
            let offset = if is_bid { -(i as i64) } else { i as i64 };
            let price = base_price + offset * 1_000000000;
            let qty = 1_000000000 + (i as i64 * 100_000_000);
            Level::new(Scale9::from_raw(price), Scale9::from_raw(qty))
        })
        .collect()
}

// Helper to setup an order book with a specific depth
fn setup_orderbook(store: &BookStore, venue: &str, inst: &str, depth: usize) {
    let bids = generate_levels(depth, 50000_000000000, true);
    let asks = generate_levels(depth, 50001_000000000, false);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    store
        .apply_snapshot(venue, inst, &bids, &asks, 1, now)
        .unwrap();
}

/// Benchmark applying snapshots at different depths
fn bench_apply_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_snapshot");
    group.throughput(Throughput::Elements(1));

    for depth in [10, 50, 100, 200] {
        let bids = generate_levels(depth, 50000_000000000, true);
        let asks = generate_levels(depth, 50001_000000000, false);

        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, _| {
            let store = BookStore::new();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;

            b.iter(|| {
                store
                    .apply_snapshot(
                        black_box("binance"),
                        black_box("BTC-USDT"),
                        black_box(&bids),
                        black_box(&asks),
                        black_box(1),
                        black_box(now),
                    )
                    .unwrap();
            });
        });
    }
    group.finish();
}

/// Benchmark applying deltas with different update sizes
fn bench_apply_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_delta");

    for update_size in [1, 5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::from_parameter(update_size),
            &update_size,
            |b, &update_size| {
                let store = BookStore::new();
                setup_orderbook(&store, "binance", "BTC-USDT", 100);

                let delta_bids = generate_levels(update_size, 49995_000000000, true);
                let delta_asks = generate_levels(update_size, 50005_000000000, false);

                b.iter(|| {
                    store
                        .apply_delta(
                            black_box("binance"),
                            black_box("BTC-USDT"),
                            black_box(&delta_bids),
                            black_box(&delta_asks),
                            black_box(2),
                            black_box(now_ns()),
                        )
                        .unwrap();
                });
            },
        );
    }
    group.finish();
}

/// Benchmark retrieving snapshots at different depths
fn bench_get_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_snapshot");

    let store = BookStore::new();
    setup_orderbook(&store, "binance", "BTC-USDT", 200);

    for depth in [5, 10, 20, 50, 100] {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| {
                store
                    .snapshot(
                        black_box("binance"),
                        black_box("BTC-USDT"),
                        black_box(depth),
                    )
                    .unwrap()
            });
        });
    }
    group.finish();
}

/// Benchmark retrieving best bid/offer
fn bench_get_bbo(c: &mut Criterion) {
    let store = BookStore::new();
    setup_orderbook(&store, "binance", "BTC-USDT", 100);

    c.bench_function("get_bbo", |b| {
        b.iter(|| {
            store
                .bbo(black_box("binance"), black_box("BTC-USDT"))
                .unwrap()
        });
    });
}

/// Benchmark concurrent reads with varying thread counts
fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_reads");
    group.sample_size(10);

    let store = Arc::new(BookStore::new());
    setup_orderbook(&store, "binance", "BTC-USDT", 100);

    for num_threads in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    let handles: Vec<_> = (0..num_threads)
                        .map(|_| {
                            let store = Arc::clone(&store);
                            thread::spawn(move || {
                                for _ in 0..1000 {
                                    let _ = store.snapshot("binance", "BTC-USDT", 20);
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

/// Benchmark write with concurrent reads
fn bench_write_with_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_with_concurrent_reads");
    group.sample_size(10);

    group.bench_function("4_readers", |b| {
        let store = Arc::new(BookStore::new());
        setup_orderbook(&store, "binance", "BTC-USDT", 100);

        // Generate delta for reuse
        let delta_bids = generate_levels(5, 49995_000000000, true);
        let delta_asks = generate_levels(5, 50005_000000000, false);

        b.iter(|| {
            // Start 4 reader threads that will read during writes
            let readers_active = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let reader_handles: Vec<_> = (0..4)
                .map(|_| {
                    let store = Arc::clone(&store);
                    let active = Arc::clone(&readers_active);
                    thread::spawn(move || {
                        let mut count = 0;
                        while active.load(std::sync::atomic::Ordering::Relaxed) {
                            let _ = store.snapshot("binance", "BTC-USDT", 20);
                            count += 1;
                            if count > 100 {
                                break;
                            }
                        }
                    })
                })
                .collect();

            // Perform 100 writes
            for seq in 2..102 {
                store
                    .apply_delta(
                        "binance",
                        "BTC-USDT",
                        &delta_bids,
                        &delta_asks,
                        seq,
                        now_ns(),
                    )
                    .unwrap();
            }

            // Signal readers to stop and wait
            readers_active.store(false, std::sync::atomic::Ordering::Relaxed);
            for h in reader_handles {
                h.join().unwrap();
            }
        });
    });

    group.finish();
}

/// Benchmark sustained throughput
fn bench_sustained_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("sustained_throughput");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    group.bench_function("100k_updates_per_sec", |b| {
        let store = BookStore::new();
        setup_orderbook(&store, "binance", "BTC-USDT", 100);

        // Pre-generate deltas to avoid generation overhead
        let delta_bids = generate_levels(2, 49998_000000000, true);
        let delta_asks = generate_levels(2, 50002_000000000, false);

        b.iter(|| {
            for seq in 2..100_002 {
                store
                    .apply_delta(
                        "binance",
                        "BTC-USDT",
                        &delta_bids,
                        &delta_asks,
                        seq,
                        now_ns(),
                    )
                    .unwrap();
            }
        });
    });
    group.finish();
}

/// Benchmark multi-instrument capacity
fn bench_multi_instrument_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_instrument_capacity");
    group.sample_size(10);

    for num_instruments in [10, 100, 500, 1000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_instruments),
            &num_instruments,
            |b, &num_instruments| {
                let store = BookStore::new();

                // Pre-populate instruments
                for i in 0..num_instruments {
                    let inst = format!("INST-{}", i);
                    setup_orderbook(&store, "binance", &inst, 50);
                }

                let delta_bids = generate_levels(5, 49995_000000000, true);
                let delta_asks = generate_levels(5, 50005_000000000, false);

                b.iter(|| {
                    // Update random instruments
                    for i in 0..100 {
                        let inst_idx = i % num_instruments;
                        let inst = format!("INST-{}", inst_idx);
                        store
                            .apply_delta("binance", &inst, &delta_bids, &delta_asks, 2, now_ns())
                            .unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

/// Benchmark staleness checking
fn bench_staleness_check(c: &mut Criterion) {
    let store = BookStore::new();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let bids = generate_levels(50, 50000_000000000, true);
    let asks = generate_levels(50, 50001_000000000, false);

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, now)
        .unwrap();

    c.bench_function("is_stale", |b| {
        b.iter(|| store.is_stale(black_box("binance"), black_box("BTC-USDT"), black_box(5000)));
    });
}

criterion_group!(
    orderbook_store_benches,
    bench_apply_snapshot,
    bench_apply_delta,
    bench_get_snapshot,
    bench_get_bbo,
    bench_concurrent_reads,
    bench_write_with_concurrent_reads,
    bench_sustained_throughput,
    bench_multi_instrument_capacity,
    bench_staleness_check
);
criterion_main!(orderbook_store_benches);

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_nanos() as u64
}
