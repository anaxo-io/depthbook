use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use orderbook::decimal::f64_to_scale9;
use orderbook::intern::InternedString;
use orderbook::types::{Book, Level, Side};

fn benchmark_level_creation(c: &mut Criterion) {
    c.bench_function("level_creation", |b| {
        b.iter(|| Level::new(black_box(50000_000000000), black_box(1_000000000)))
    });
}

fn benchmark_side_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("side_insert");

    for count in [1, 10, 50, 100, 200].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter(|| {
                let mut bids = Side::new();
                for i in 0..count {
                    let price = (50000 - i) * 1_000000000;
                    bids.insert(Level::new(price, 1_000000000), true);
                }
                black_box(bids)
            })
        });
    }
    group.finish();
}

fn benchmark_side_update(c: &mut Criterion) {
    let mut bids = Side::new();
    for i in 0..100 {
        let price = (50000 - i) * 1_000000000;
        bids.insert(Level::new(price, 1_000000000), true);
    }

    c.bench_function("side_update", |b| {
        let mut bids_clone = bids.clone();
        b.iter(|| {
            bids_clone.update(black_box(49950_000000000), black_box(2_000000000), true);
        })
    });
}

fn benchmark_side_remove(c: &mut Criterion) {
    c.bench_function("side_remove", |b| {
        b.iter_batched(
            || {
                let mut bids = Side::new();
                for i in 0..100 {
                    let price = (50000 - i) * 1_000000000;
                    bids.insert(Level::new(price, 1_000000000), true);
                }
                bids
            },
            |mut bids| {
                bids.remove(black_box(49950_000000000), true);
                black_box(bids)
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn benchmark_side_best(c: &mut Criterion) {
    let mut bids = Side::new();
    for i in 0..100 {
        let price = (50000 - i) * 1_000000000;
        bids.insert(Level::new(price, 1_000000000), true);
    }

    c.bench_function("side_best", |b| b.iter(|| black_box(bids.best())));
}

fn benchmark_side_depth_within_bps(c: &mut Criterion) {
    let mut bids = Side::new();
    for i in 0..100 {
        let price = (50000 - i * 10) * 1_000000000;
        bids.insert(Level::new(price, 1_000000000), true);
    }

    c.bench_function("side_depth_within_bps", |b| {
        b.iter(|| black_box(bids.depth_within_bps(black_box(200), true)))
    });
}

fn benchmark_snapshot_creation(c: &mut Criterion) {
    c.bench_function("snapshot_creation", |b| {
        b.iter(|| {
            Book::new(
                black_box(InternedString::new("binance")),
                black_box(InternedString::new("BTC-USDT")),
                black_box(1234567890000000000),
                black_box(42),
            )
        })
    });
}

fn benchmark_snapshot_mid_price(c: &mut Criterion) {
    let mut snapshot = Book::new(
        InternedString::new("binance"),
        InternedString::new("BTC-USDT"),
        1234567890000000000,
        42,
    );
    snapshot
        .bids
        .insert(Level::new(50000_000000000, 1_000000000), true);
    snapshot
        .asks
        .insert(Level::new(50010_000000000, 1_000000000), false);

    c.bench_function("snapshot_mid_price", |b| {
        b.iter(|| black_box(snapshot.mid_price()))
    });
}

fn benchmark_snapshot_to_json(c: &mut Criterion) {
    let mut snapshot = Book::new(
        InternedString::new("binance"),
        InternedString::new("BTC-USDT"),
        1234567890000000000,
        42,
    );

    for i in 0..50 {
        let bid_price = (50000 - i) * 1_000000000;
        let ask_price = (50010 + i) * 1_000000000;
        snapshot
            .bids
            .insert(Level::new(bid_price, 1_000000000), true);
        snapshot
            .asks
            .insert(Level::new(ask_price, 1_000000000), false);
    }

    c.bench_function("snapshot_to_json_50_levels", |b| {
        b.iter(|| black_box(snapshot.to_json().unwrap()))
    });
}

fn benchmark_interned_string_creation(c: &mut Criterion) {
    c.bench_function("interned_string_new", |b| {
        b.iter(|| black_box(InternedString::new("binance")))
    });
}

fn benchmark_interned_string_dedup(c: &mut Criterion) {
    // Pre-create the string to ensure it's in the cache
    let _ = InternedString::new("cached_binance");

    c.bench_function("interned_string_cached", |b| {
        b.iter(|| black_box(InternedString::new("cached_binance")))
    });
}

fn benchmark_decimal_conversion(c: &mut Criterion) {
    c.bench_function("f64_to_scale9", |b| {
        b.iter(|| black_box(f64_to_scale9(black_box(123.456789012))))
    });
}

fn benchmark_realistic_order_book_updates(c: &mut Criterion) {
    c.bench_function("realistic_100_updates", |b| {
        b.iter_batched(
            || {
                let mut snapshot = Book::new(
                    InternedString::new("binance"),
                    InternedString::new("BTC-USDT"),
                    1234567890000000000,
                    0,
                );
                // Initialize with 50 levels on each side
                for i in 0..50 {
                    let bid_price = (50000 - i) * 1_000000000;
                    let ask_price = (50010 + i) * 1_000000000;
                    snapshot
                        .bids
                        .insert(Level::new(bid_price, 1_000000000), true);
                    snapshot
                        .asks
                        .insert(Level::new(ask_price, 1_000000000), false);
                }
                snapshot
            },
            |mut snapshot| {
                // Simulate 100 updates
                for i in 0..100 {
                    let price = (50000 - (i % 50)) * 1_000000000;
                    let qty = ((i % 10) + 1) * 100_000_000;
                    snapshot.bids.update(price, qty, true);
                }
                black_box(snapshot)
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    benchmark_level_creation,
    benchmark_side_insert,
    benchmark_side_update,
    benchmark_side_remove,
    benchmark_side_best,
    benchmark_side_depth_within_bps,
    benchmark_snapshot_creation,
    benchmark_snapshot_mid_price,
    benchmark_snapshot_to_json,
    benchmark_interned_string_creation,
    benchmark_interned_string_dedup,
    benchmark_decimal_conversion,
    benchmark_realistic_order_book_updates
);
criterion_main!(benches);
