//! Concurrent store holding one [`Book`] per venue and instrument.

use crate::book_state::BookState;
use crate::error::{Error, Result};
use crate::intern::InternedString;
use crate::types::{Book, Level};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Counters describing what a [`BookStore`] has seen since it was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Number of books currently tracked.
    pub books: usize,
    /// Number of deltas rejected because they skipped a sequence number.
    pub sequence_gaps: u64,
    /// Number of snapshots applied.
    pub snapshots_applied: u64,
    /// Number of deltas applied.
    pub deltas_applied: u64,
}

/// Stores order books keyed by venue and instrument.
///
/// Books are held in a [`DashMap`], so updates to different instruments proceed
/// independently. Each book is guarded by a read-write lock: any number of readers may
/// hold a book while no writer does. Reads take the lock without blocking where possible
/// and fall back to a blocking acquire under write contention.
///
/// The intended usage is one writer per instrument — typically the task that owns the
/// exchange connection — with any number of concurrent readers.
///
/// # Example
///
/// ```
/// use orderbook::{BookStore, Level, f64_to_scale9};
///
/// let store = BookStore::new();
/// store.apply_snapshot(
///     "binance",
///     "BTC-USDT",
///     &[Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0))],
///     &[Level::new(f64_to_scale9(50_010.0), f64_to_scale9(1.0))],
///     1,
///     1_700_000_000_000_000_000,
/// )?;
///
/// let book = store.snapshot("binance", "BTC-USDT", 10).unwrap();
/// assert_eq!(book.spread(), Some(f64_to_scale9(10.0)));
/// # Ok::<(), orderbook::Error>(())
/// ```
#[derive(Debug, Default)]
pub struct BookStore {
    books: DashMap<(String, String), Arc<BookState>>,
    sequence_gaps: AtomicU64,
    snapshots_applied: AtomicU64,
    deltas_applied: AtomicU64,
}

impl BookStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace a book with a full snapshot.
    ///
    /// Creates the book if it does not exist. Levels with zero quantity are skipped.
    /// A snapshot always wins: it does not consult the current sequence number, which is
    /// what makes it usable for recovery after a gap.
    ///
    /// `ts` is the exchange timestamp in nanoseconds since the Unix epoch.
    pub fn apply_snapshot(
        &self,
        venue: &str,
        inst: &str,
        bids: &[Level],
        asks: &[Level],
        seq: u64,
        ts: u64,
    ) -> Result<()> {
        let mut book = Book::new(
            InternedString::new(venue),
            InternedString::new(inst),
            ts,
            seq,
        );

        for level in bids {
            if !level.is_empty() {
                book.bids.insert(*level, true);
            }
        }
        for level in asks {
            if !level.is_empty() {
                book.asks.insert(*level, false);
            }
        }

        let state = self.get_or_create(venue, inst, seq, ts);
        state.replace(book, ts);
        self.snapshots_applied.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Apply an incremental update to an existing book.
    ///
    /// A level with zero quantity removes that price. `ts` is the exchange timestamp in
    /// nanoseconds since the Unix epoch.
    ///
    /// # Errors
    ///
    /// - [`Error::NotFound`] if no snapshot has been applied for this venue and instrument.
    /// - [`Error::SequenceGap`] if `seq` skips ahead of the book's sequence number. The
    ///   update is **not** applied; the caller should re-request a snapshot. The gap is
    ///   counted in [`Stats::sequence_gaps`].
    ///
    /// An update whose sequence number is already known is ignored and returns `Ok`.
    pub fn apply_delta(
        &self,
        venue: &str,
        inst: &str,
        bids: &[Level],
        asks: &[Level],
        seq: u64,
        ts: u64,
    ) -> Result<()> {
        let key = (venue.to_string(), inst.to_string());
        let state = self.books.get(&key).ok_or_else(|| Error::NotFound {
            venue: venue.to_string(),
            inst: inst.to_string(),
        })?;

        let current_seq = state.sequence();
        if seq > current_seq + 1 {
            self.sequence_gaps.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                venue,
                inst,
                expected = current_seq + 1,
                received = seq,
                "sequence gap; book needs a fresh snapshot"
            );
            return Err(Error::SequenceGap {
                expected: current_seq + 1,
                received: seq,
            });
        }

        if seq <= current_seq {
            return Ok(());
        }

        state.apply(
            |book| {
                for level in bids {
                    book.bids.insert(*level, true);
                }
                for level in asks {
                    book.asks.insert(*level, false);
                }
            },
            seq,
            ts,
        );
        self.deltas_applied.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Get a copy of a book, keeping at most `depth` levels per side.
    ///
    /// `depth` of `0` returns every level. Returns `None` if the book does not exist.
    pub fn snapshot(&self, venue: &str, inst: &str, depth: usize) -> Option<Book> {
        let key = (venue.to_string(), inst.to_string());
        let state = self.books.get(&key)?;

        let mut book = state.snapshot();
        if depth > 0 {
            book.bids.truncate(depth);
            book.asks.truncate(depth);
        }
        Some(book)
    }

    /// Get the best bid and best ask.
    ///
    /// Returns `None` if the book does not exist or either side is empty. This is cheaper
    /// than [`BookStore::snapshot`], which copies the whole book.
    pub fn bbo(&self, venue: &str, inst: &str) -> Option<(Level, Level)> {
        let key = (venue.to_string(), inst.to_string());
        self.books.get(&key)?.bbo()
    }

    /// Report whether a book is older than `max_age_ms`.
    ///
    /// A book that does not exist counts as stale.
    pub fn is_stale(&self, venue: &str, inst: &str, max_age_ms: u64) -> bool {
        let key = (venue.to_string(), inst.to_string());
        match self.books.get(&key) {
            Some(state) => state.is_stale(max_age_ms),
            None => true,
        }
    }

    /// Current counters. See [`Stats`].
    pub fn stats(&self) -> Stats {
        Stats {
            books: self.books.len(),
            sequence_gaps: self.sequence_gaps.load(Ordering::Relaxed),
            snapshots_applied: self.snapshots_applied.load(Ordering::Relaxed),
            deltas_applied: self.deltas_applied.load(Ordering::Relaxed),
        }
    }

    fn get_or_create(&self, venue: &str, inst: &str, seq: u64, ts: u64) -> Arc<BookState> {
        let key = (venue.to_string(), inst.to_string());
        self.books
            .entry(key)
            .or_insert_with(|| {
                let book = Book::new(
                    InternedString::new(venue),
                    InternedString::new(inst),
                    ts,
                    seq,
                );
                Arc::new(BookState::new(book))
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::f64_to_scale9;

    const TS: u64 = 1_700_000_000_000_000_000;

    #[test]
    fn apply_snapshot_populates_both_sides() {
        let store = BookStore::new();

        let bids = vec![
            Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)),
            Level::new(f64_to_scale9(49999.0), f64_to_scale9(2.0)),
        ];
        let asks = vec![
            Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.5)),
            Level::new(f64_to_scale9(50002.0), f64_to_scale9(2.5)),
        ];

        store
            .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, TS)
            .unwrap();

        let book = store.snapshot("binance", "BTC-USDT", 0).unwrap();
        assert_eq!(book.bids.count(), 2);
        assert_eq!(book.asks.count(), 2);
        assert_eq!(store.stats().snapshots_applied, 1);
    }

    #[test]
    fn apply_delta_adds_a_level() {
        let store = BookStore::new();
        let bids = vec![Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0))];
        let asks = vec![Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0))];

        store
            .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, TS)
            .unwrap();

        let delta = vec![Level::new(f64_to_scale9(49999.0), f64_to_scale9(2.0))];
        store
            .apply_delta("binance", "BTC-USDT", &delta, &[], 2, TS + 1)
            .unwrap();

        let book = store.snapshot("binance", "BTC-USDT", 0).unwrap();
        assert_eq!(book.bids.count(), 2);
        assert_eq!(store.stats().deltas_applied, 1);
    }

    #[test]
    fn delta_with_a_gap_is_rejected_and_counted() {
        let store = BookStore::new();
        store
            .apply_snapshot("binance", "BTC-USDT", &[], &[], 1, TS)
            .unwrap();

        let result = store.apply_delta("binance", "BTC-USDT", &[], &[], 5, TS + 1);

        assert!(matches!(
            result,
            Err(Error::SequenceGap {
                expected: 2,
                received: 5
            })
        ));
        assert_eq!(store.stats().sequence_gaps, 1);
    }

    #[test]
    fn delta_keeps_the_exchange_timestamp() {
        let store = BookStore::new();
        store
            .apply_snapshot("binance", "BTC-USDT", &[], &[], 1, TS)
            .unwrap();
        store
            .apply_delta("binance", "BTC-USDT", &[], &[], 2, TS + 42)
            .unwrap();

        let book = store.snapshot("binance", "BTC-USDT", 0).unwrap();
        assert_eq!(book.ts, TS + 42);
    }

    #[test]
    fn snapshot_honours_depth() {
        let store = BookStore::new();
        let bids: Vec<Level> = (0..20)
            .map(|i| Level::new(f64_to_scale9(50000.0 - i as f64), f64_to_scale9(1.0)))
            .collect();
        let asks: Vec<Level> = (0..20)
            .map(|i| Level::new(f64_to_scale9(50001.0 + i as f64), f64_to_scale9(1.0)))
            .collect();

        store
            .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, TS)
            .unwrap();

        let shallow = store.snapshot("binance", "BTC-USDT", 5).unwrap();
        assert_eq!(shallow.bids.count(), 5);
        assert_eq!(shallow.asks.count(), 5);
        // Truncation keeps the best levels.
        assert_eq!(shallow.bids.best().unwrap().price, f64_to_scale9(50000.0));

        let full = store.snapshot("binance", "BTC-USDT", 0).unwrap();
        assert_eq!(full.bids.count(), 20);
    }

    #[test]
    fn bbo_returns_top_of_book() {
        let store = BookStore::new();
        let bids = vec![
            Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)),
            Level::new(f64_to_scale9(49999.0), f64_to_scale9(2.0)),
        ];
        let asks = vec![
            Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.5)),
            Level::new(f64_to_scale9(50002.0), f64_to_scale9(2.5)),
        ];

        store
            .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, TS)
            .unwrap();

        let (bid, ask) = store.bbo("binance", "BTC-USDT").unwrap();
        assert_eq!(bid.price, f64_to_scale9(50000.0));
        assert_eq!(ask.price, f64_to_scale9(50001.0));
    }

    #[test]
    fn missing_book_is_stale_and_not_found() {
        let store = BookStore::new();

        assert!(store.is_stale("binance", "BTC-USDT", 1000));
        assert!(matches!(
            store.apply_delta("binance", "BTC-USDT", &[], &[], 1, TS),
            Err(Error::NotFound { .. })
        ));
    }

    #[test]
    fn fresh_book_is_not_stale() {
        let store = BookStore::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_nanos() as u64;

        store
            .apply_snapshot("binance", "BTC-USDT", &[], &[], 1, now)
            .unwrap();

        assert!(!store.is_stale("binance", "BTC-USDT", 1000));
    }

    #[test]
    fn old_delta_is_ignored() {
        let store = BookStore::new();
        store
            .apply_snapshot("binance", "BTC-USDT", &[], &[], 5, TS)
            .unwrap();

        assert!(store
            .apply_delta("binance", "BTC-USDT", &[], &[], 3, TS + 1)
            .is_ok());
        assert_eq!(store.stats().deltas_applied, 0);
    }
}
