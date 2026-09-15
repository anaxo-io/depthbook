//! Experiment: best-at-front vs best-at-back layout, binary vs linear search.
//!
//! Four local `Side` variants, bid semantics only, exercised with a top-heavy update
//! stream. See the "When Nanoseconds Matter" (D. Gross, CppCon 2024) reversed-vector
//! and linear-search slides for the motivation.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use orderbook::{Level, Scale9, MAX_LEVELS};
use std::collections::BTreeMap;
use std::hint::black_box;

/// A bid ladder: absolute-quantity upsert, zero deletes, best is the highest price.
trait Ladder: Clone {
    fn empty() -> Self;
    fn insert(&mut self, level: Level);
    fn best(&self) -> Option<Level>;
}

/// Standard-library baseline: the "first take" of most implementations.
impl Ladder for BTreeMap<Scale9, Scale9> {
    fn empty() -> Self {
        BTreeMap::new()
    }
    fn insert(&mut self, level: Level) {
        if level.qty.is_zero() {
            self.remove(&level.price);
        } else {
            BTreeMap::insert(self, level.price, level.qty);
        }
    }
    fn best(&self) -> Option<Level> {
        self.last_key_value().map(|(p, q)| Level::new(*p, *q))
    }
}

/// Standard-library baseline: a growable sorted vector, best-first, binary search.
impl Ladder for Vec<Level> {
    fn empty() -> Self {
        Vec::new()
    }
    fn insert(&mut self, level: Level) {
        let pos = self.partition_point(|l| l.price > level.price);
        if pos < self.len() && self[pos].price == level.price {
            if level.qty.is_zero() {
                self.remove(pos);
            } else {
                self[pos] = level;
            }
        } else if !level.qty.is_zero() {
            Vec::insert(self, pos, level);
        }
    }
    fn best(&self) -> Option<Level> {
        self.first().copied()
    }
}

impl<const REV: bool, const LIN: usize> Ladder for Side<REV, LIN> {
    fn empty() -> Self {
        Side::new()
    }
    fn insert(&mut self, level: Level) {
        Side::insert(self, level)
    }
    fn best(&self) -> Option<Level> {
        Side::best(self)
    }
}

/// `REV`: best level at the end of the array (so near-best inserts move little).
/// `LIN`: 0 = binary search, `usize::MAX` = linear scan from the best end, otherwise a
/// hybrid that scans that many levels from the best end and binary-searches the rest.
#[derive(Clone)]
struct Side<const REV: bool, const LIN: usize> {
    levels: [Level; MAX_LEVELS],
    count: usize,
}

impl<const REV: bool, const LIN: usize> Side<REV, LIN> {
    fn new() -> Self {
        Self {
            levels: [Level::new(Scale9::ZERO, Scale9::ZERO); MAX_LEVELS],
            count: 0,
        }
    }

    /// Slot where `price` belongs. Storage order is descending price when `!REV`,
    /// ascending when `REV`, so the best bid is at index 0 or `count - 1`.
    #[inline]
    fn position(&self, price: Scale9) -> usize {
        let active = &self.levels[..self.count];
        if LIN == 0 {
            return if REV {
                active
                    .binary_search_by(|l| l.price.cmp(&price))
                    .unwrap_or_else(|p| p)
            } else {
                active
                    .binary_search_by(|l| price.cmp(&l.price))
                    .unwrap_or_else(|p| p)
            };
        }
        if REV {
            // Scan from the best end (back) toward the front, at most LIN levels.
            let scan_from = self.count.saturating_sub(LIN);
            match active[scan_from..].iter().rposition(|l| l.price < price) {
                Some(i) => scan_from + i + 1,
                None if scan_from == 0 => 0,
                None => active[..scan_from]
                    .binary_search_by(|l| l.price.cmp(&price))
                    .unwrap_or_else(|p| p),
            }
        } else {
            let scan_to = self.count.min(LIN);
            match active[..scan_to].iter().position(|l| l.price <= price) {
                Some(i) => i,
                None if scan_to == self.count => self.count,
                None => {
                    scan_to
                        + active[scan_to..]
                            .binary_search_by(|l| price.cmp(&l.price))
                            .unwrap_or_else(|p| p)
                }
            }
        }
    }

    fn insert(&mut self, level: Level) {
        let pos = self.position(level.price);
        if pos < self.count && self.levels[pos].price == level.price {
            if level.qty.is_zero() {
                self.levels.copy_within(pos + 1..self.count, pos);
                self.count -= 1;
            } else {
                self.levels[pos] = level;
            }
            return;
        }
        if level.qty.is_zero() {
            return;
        }
        if self.count == MAX_LEVELS {
            if REV {
                // Worst level is at the front: drop it by shifting everything down.
                if pos == 0 {
                    return;
                }
                self.levels.copy_within(1..pos, 0);
                self.levels[pos - 1] = level;
                return;
            }
            if pos == MAX_LEVELS {
                return;
            }
            self.count -= 1;
        }
        self.levels.copy_within(pos..self.count, pos + 1);
        self.levels[pos] = level;
        self.count += 1;
    }

    #[inline]
    fn best(&self) -> Option<Level> {
        if self.count == 0 {
            None
        } else if REV {
            Some(self.levels[self.count - 1])
        } else {
            Some(self.levels[0])
        }
    }
}

const TICK: i64 = 1_000_000_000;
const TOP: i64 = 50_000 * TICK;

fn filled<L: Ladder>(depth: usize) -> L {
    let mut s = L::empty();
    for i in 0..depth as i64 {
        s.insert(Level::new(Scale9::from_raw(TOP - i * TICK), Scale9::ONE));
    }
    assert_eq!(s.best().unwrap().price.raw(), TOP);
    s
}

/// Deterministic top-heavy stream: rank from the best is geometric (p = 0.5), a fifth of
/// updates are deletes, and deleted levels are re-added by later updates at the same price.
fn stream(n: usize) -> Vec<Level> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..n)
        .map(|_| {
            let rank = (next().trailing_zeros() as i64).min(30);
            let qty = if next() % 5 == 0 {
                0
            } else {
                1 + (next() % 9) as i64
            };
            Level::new(
                Scale9::from_raw(TOP - rank * TICK),
                Scale9::from_raw(qty * TICK),
            )
        })
        .collect()
}

fn run<L: Ladder>(c: &mut Criterion, name: &str) {
    for depth in [50usize, 200] {
        let base = filled::<L>(depth);
        let updates = stream(1_000);

        let mut g = c.benchmark_group(format!("layout/{name}"));
        g.throughput(Throughput::Elements(updates.len() as u64));
        g.bench_with_input(BenchmarkId::new("stream", depth), &depth, |b, _| {
            let mut s = base.clone();
            b.iter(|| {
                for u in &updates {
                    s.insert(black_box(*u));
                }
                black_box(s.best())
            })
        });
        g.finish();

        let mut g = c.benchmark_group(format!("layout/{name}"));
        g.bench_with_input(BenchmarkId::new("update_best", depth), &depth, |b, _| {
            let mut s = base.clone();
            let l = Level::new(Scale9::from_raw(TOP), Scale9::from_raw(2 * TICK));
            b.iter(|| s.insert(black_box(l)))
        });
        // Insert a new level one tick inside the best, then remove it again.
        g.bench_with_input(BenchmarkId::new("add_remove_top", depth), &depth, |b, _| {
            let mut s = base.clone();
            let add = Level::new(Scale9::from_raw(TOP - TICK / 2), Scale9::ONE);
            let del = Level::new(add.price, Scale9::ZERO);
            b.iter(|| {
                s.insert(black_box(add));
                s.insert(black_box(del));
            })
        });
        // Same, in the middle of the book.
        g.bench_with_input(BenchmarkId::new("add_remove_mid", depth), &depth, |b, _| {
            let mut s = base.clone();
            let mid = TOP - (depth as i64 / 2) * TICK - TICK / 2;
            let add = Level::new(Scale9::from_raw(mid), Scale9::ONE);
            let del = Level::new(add.price, Scale9::ZERO);
            b.iter(|| {
                s.insert(black_box(add));
                s.insert(black_box(del));
            })
        });
        g.finish();
    }
}

fn all(c: &mut Criterion) {
    run::<BTreeMap<Scale9, Scale9>>(c, "std_btreemap");
    run::<Vec<Level>>(c, "std_vec");
    run::<Side<false, 0>>(c, "front_binary");
    run::<Side<true, 0>>(c, "back_binary");
    run::<Side<false, { usize::MAX }>>(c, "front_linear");
    run::<Side<true, { usize::MAX }>>(c, "back_linear");
    run::<Side<true, 16>>(c, "back_hybrid16");
}

criterion_group!(benches, all);
criterion_main!(benches);
