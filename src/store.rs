//! Concurrent store holding one [`Book`] per venue and instrument.

use crate::book_state::BookState;
use crate::error::{Error, Result};
use crate::intern::InternedString;
use crate::types::{Book, Insertion, Level};
use dashmap::DashMap;
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Owned map key: venue and instrument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key(String, String);

/// Lets the map be probed with two `&str` without building a `Key`, which would cost two
/// allocations on every delta. `Key` and `(&str, &str)` hash and compare identically
/// through this trait, so `DashMap::get(&(venue, inst) as &dyn KeyLike)` finds the entry.
trait KeyLike {
    fn venue(&self) -> &str;
    fn inst(&self) -> &str;
}

impl KeyLike for Key {
    fn venue(&self) -> &str {
        &self.0
    }
    fn inst(&self) -> &str {
        &self.1
    }
}

impl KeyLike for (&str, &str) {
    fn venue(&self) -> &str {
        self.0
    }
    fn inst(&self) -> &str {
        self.1
    }
}

impl Hash for dyn KeyLike + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.venue().hash(state);
        self.inst().hash(state);
    }
}

impl PartialEq for dyn KeyLike + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.venue() == other.venue() && self.inst() == other.inst()
    }
}

impl Eq for dyn KeyLike + '_ {}

impl<'a> Borrow<dyn KeyLike + 'a> for Key {
    fn borrow(&self) -> &(dyn KeyLike + 'a) {
        self
    }
}

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
    /// Number of snapshots rejected for being older than the book they would replace.
    pub snapshots_rejected: u64,
    /// Number of price levels discarded because a side was already full.
    ///
    /// Non-zero means a book is no longer a faithful copy of the venue's below the top
    /// [`MAX_LEVELS`](crate::MAX_LEVELS) levels. Harmless for top-of-book work, and the
    /// reason this crate is the wrong choice for full-depth archival.
    pub levels_dropped: u64,
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
/// use depthbook::{BookStore, Level, f64_to_scale9};
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
/// # Ok::<(), depthbook::Error>(())
/// ```
#[derive(Debug, Default)]
pub struct BookStore {
    books: DashMap<Key, Arc<BookState>>,
    sequence_gaps: AtomicU64,
    snapshots_applied: AtomicU64,
    deltas_applied: AtomicU64,
    snapshots_rejected: AtomicU64,
    levels_dropped: AtomicU64,
}

/// Reject levels no feed can legitimately send.
///
/// Quantity may be zero, which deletes the level in a delta, but not negative. Price may
/// not be negative.
fn validate(levels: &[Level]) -> Result<()> {
    for level in levels {
        if level.qty.raw() < 0 || level.price.raw() < 0 {
            return Err(Error::InvalidData(format!(
                "negative price or quantity: {} @ {}",
                level.qty, level.price
            )));
        }
    }
    Ok(())
}

impl BookStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace a book with a full snapshot and clear its gapped flag.
    ///
    /// Creates the book if it does not exist. Levels with zero quantity are skipped.
    /// A snapshot does not need to be the next sequence number, which is what makes it
    /// usable for recovery after a gap, but it may not be older than the book.
    ///
    /// `ts` is the exchange timestamp in nanoseconds since the Unix epoch.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidData`] if any level has a negative price or quantity.
    /// - [`Error::OutOfOrder`] if `seq` is below the book's current sequence number. The
    ///   snapshot is **not** applied and is counted in [`Stats::snapshots_rejected`]. If
    ///   the venue reset its sequence numbers, call [`BookStore::remove`] first.
    pub fn apply_snapshot(
        &self,
        venue: &str,
        inst: &str,
        bids: &[Level],
        asks: &[Level],
        seq: u64,
        ts: u64,
    ) -> Result<()> {
        validate(bids)?;
        validate(asks)?;

        let mut book = Book::new(
            InternedString::new(venue),
            InternedString::new(inst),
            ts,
            seq,
        );

        // Feeds send snapshots best-first. Levels are stored best-last, so inserting in
        // reverse appends each one instead of shifting the whole array.
        let mut dropped = 0u64;
        for level in bids.iter().rev() {
            if !level.is_empty() && book.bids.insert_outcome(*level, true) == Insertion::Dropped {
                dropped += 1;
            }
        }
        for level in asks.iter().rev() {
            if !level.is_empty() && book.asks.insert_outcome(*level, false) == Insertion::Dropped {
                dropped += 1;
            }
        }
        self.record_drops(venue, inst, dropped);

        let state = self.get_or_create(venue, inst, seq, ts);
        state.replace(book, ts).inspect_err(|e| {
            self.snapshots_rejected.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(venue, inst, error = %e, "snapshot rejected");
        })?;
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
    /// - [`Error::InvalidData`] if any level has a negative price or quantity.
    /// - [`Error::NotFound`] if no snapshot has been applied for this venue and instrument.
    /// - [`Error::SequenceGap`] if `seq` skips ahead of the book's sequence number. The
    ///   update is **not** applied and the book is flagged as gapped (see
    ///   [`BookStore::is_gapped`]) until the next snapshot. The gap is counted in
    ///   [`Stats::sequence_gaps`].
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
        validate(bids)?;
        validate(asks)?;

        let state = self
            .books
            .get(&(venue, inst) as &dyn KeyLike)
            .ok_or_else(|| Error::NotFound {
                venue: venue.to_string(),
                inst: inst.to_string(),
            })?;

        // Counted inside the closure but reported outside it, so the write lock is not
        // held across the atomic and the tracing call.
        let mut dropped = 0u64;
        let applied = state
            .apply(
                |book| {
                    for level in bids {
                        if book.bids.insert_outcome(*level, true) == Insertion::Dropped {
                            dropped += 1;
                        }
                    }
                    for level in asks {
                        if book.asks.insert_outcome(*level, false) == Insertion::Dropped {
                            dropped += 1;
                        }
                    }
                },
                seq,
                ts,
            )
            .inspect_err(|e| {
                if let Error::SequenceGap { expected, received } = e {
                    self.sequence_gaps.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        venue,
                        inst,
                        expected,
                        received,
                        "sequence gap; book needs a fresh snapshot"
                    );
                }
            })?;
        if !applied {
            return Ok(());
        }
        self.record_drops(venue, inst, dropped);
        self.deltas_applied.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Get a copy of a book, keeping at most `depth` levels per side.
    ///
    /// `depth` of `0` returns every level. Returns `None` if the book does not exist.
    pub fn snapshot(&self, venue: &str, inst: &str, depth: usize) -> Option<Book> {
        let state = self.books.get(&(venue, inst) as &dyn KeyLike)?;

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
        self.books.get(&(venue, inst) as &dyn KeyLike)?.bbo()
    }

    /// Report whether a book is older than `max_age_ms`.
    ///
    /// A book that does not exist counts as stale.
    pub fn is_stale(&self, venue: &str, inst: &str, max_age_ms: u64) -> bool {
        match self.books.get(&(venue, inst) as &dyn KeyLike) {
            Some(state) => state.is_stale(max_age_ms),
            None => true,
        }
    }

    /// Report whether a book has rejected a delta for a sequence gap since its last
    /// snapshot.
    ///
    /// A gapped book still serves its last good state; this is how a reader finds out
    /// that state is behind the venue. A book that does not exist counts as gapped.
    pub fn is_gapped(&self, venue: &str, inst: &str) -> bool {
        match self.books.get(&(venue, inst) as &dyn KeyLike) {
            Some(state) => state.is_gapped(),
            None => true,
        }
    }

    /// Forget a book, returning whether it existed.
    ///
    /// Call this before the first snapshot of a new sequence when a venue restarts its
    /// numbering, for example after a reconnect, or when an instrument is delisted.
    pub fn remove(&self, venue: &str, inst: &str) -> bool {
        self.books.remove(&(venue, inst) as &dyn KeyLike).is_some()
    }

    /// Count levels lost to a full side, and say so once per batch rather than per level.
    fn record_drops(&self, venue: &str, inst: &str, dropped: u64) {
        if dropped == 0 {
            return;
        }
        self.levels_dropped.fetch_add(dropped, Ordering::Relaxed);
        tracing::debug!(
            venue,
            inst,
            dropped,
            "side full; deepest levels discarded to keep the book sorted"
        );
    }

    /// Current counters. See [`Stats`].
    pub fn stats(&self) -> Stats {
        Stats {
            books: self.books.len(),
            sequence_gaps: self.sequence_gaps.load(Ordering::Relaxed),
            snapshots_applied: self.snapshots_applied.load(Ordering::Relaxed),
            deltas_applied: self.deltas_applied.load(Ordering::Relaxed),
            snapshots_rejected: self.snapshots_rejected.load(Ordering::Relaxed),
            levels_dropped: self.levels_dropped.load(Ordering::Relaxed),
        }
    }

    fn get_or_create(&self, venue: &str, inst: &str, seq: u64, ts: u64) -> Arc<BookState> {
        self.books
            .entry(Key(venue.to_string(), inst.to_string()))
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

    fn one(price: f64) -> Vec<Level> {
        vec![Level::new(f64_to_scale9(price), f64_to_scale9(1.0))]
    }

    #[test]
    fn gap_flags_the_book_until_a_snapshot_clears_it() {
        let store = BookStore::new();
        assert!(store.is_gapped("binance", "BTC-USDT"));

        store
            .apply_snapshot("binance", "BTC-USDT", &one(1.0), &one(2.0), 1, TS)
            .unwrap();
        assert!(!store.is_gapped("binance", "BTC-USDT"));

        assert!(store
            .apply_delta("binance", "BTC-USDT", &[], &[], 5, TS)
            .is_err());
        assert!(store.is_gapped("binance", "BTC-USDT"));
        assert!(store.snapshot("binance", "BTC-USDT", 0).unwrap().gapped);
        // Still serving the last good state.
        assert!(store.bbo("binance", "BTC-USDT").is_some());

        store
            .apply_snapshot("binance", "BTC-USDT", &one(1.0), &one(2.0), 5, TS)
            .unwrap();
        assert!(!store.is_gapped("binance", "BTC-USDT"));
        assert!(!store.snapshot("binance", "BTC-USDT", 0).unwrap().gapped);
    }

    #[test]
    fn older_snapshot_is_rejected_until_the_book_is_removed() {
        let store = BookStore::new();
        store
            .apply_snapshot("binance", "BTC-USDT", &one(1.0), &one(2.0), 10, TS)
            .unwrap();

        let err = store
            .apply_snapshot("binance", "BTC-USDT", &one(3.0), &one(4.0), 9, TS)
            .unwrap_err();
        assert_eq!(
            err,
            Error::OutOfOrder {
                current: 10,
                received: 9
            }
        );
        assert!(err.to_string().contains("call remove first"));
        assert_eq!(store.stats().snapshots_rejected, 1);
        assert_eq!(store.snapshot("binance", "BTC-USDT", 0).unwrap().seq, 10);

        // Same sequence is a harmless re-snapshot.
        store
            .apply_snapshot("binance", "BTC-USDT", &one(1.0), &one(2.0), 10, TS)
            .unwrap();

        assert!(store.remove("binance", "BTC-USDT"));
        assert!(!store.remove("binance", "BTC-USDT"));
        store
            .apply_snapshot("binance", "BTC-USDT", &one(3.0), &one(4.0), 1, TS)
            .unwrap();
        assert_eq!(store.snapshot("binance", "BTC-USDT", 0).unwrap().seq, 1);
    }

    #[test]
    fn dropped_levels_are_counted() {
        let store = BookStore::new();
        let bids: Vec<Level> = (0..crate::MAX_LEVELS + 5)
            .map(|i| Level::new(f64_to_scale9(50_000.0 - i as f64), f64_to_scale9(1.0)))
            .collect();

        // A snapshot deeper than the side can hold loses its worst levels.
        store
            .apply_snapshot("binance", "BTC-USDT", &bids, &one(60_000.0), 1, TS)
            .unwrap();
        assert_eq!(store.stats().levels_dropped, 5);
        let book = store.snapshot("binance", "BTC-USDT", 0).unwrap();
        assert_eq!(book.bids.count(), crate::MAX_LEVELS);
        assert_eq!(book.bids.best().unwrap().price, f64_to_scale9(50_000.0));

        // A delta adding a new level to a full side drops another one.
        store
            .apply_delta(
                "binance",
                "BTC-USDT",
                &[Level::new(f64_to_scale9(50_001.0), f64_to_scale9(1.0))],
                &[],
                2,
                TS,
            )
            .unwrap();
        assert_eq!(store.stats().levels_dropped, 6);

        // Updating a level that is already there is not a drop.
        store
            .apply_delta(
                "binance",
                "BTC-USDT",
                &[Level::new(f64_to_scale9(50_001.0), f64_to_scale9(3.0))],
                &[],
                3,
                TS,
            )
            .unwrap();
        assert_eq!(store.stats().levels_dropped, 6);
    }

    #[test]
    fn negative_prices_and_quantities_are_rejected() {
        let store = BookStore::new();
        let bad_qty = vec![Level::new(f64_to_scale9(1.0), f64_to_scale9(-1.0))];
        let bad_price = vec![Level::new(f64_to_scale9(-1.0), f64_to_scale9(1.0))];

        assert!(matches!(
            store.apply_snapshot("binance", "BTC-USDT", &bad_qty, &[], 1, TS),
            Err(Error::InvalidData(_))
        ));
        assert!(store.snapshot("binance", "BTC-USDT", 0).is_none());

        store
            .apply_snapshot("binance", "BTC-USDT", &one(1.0), &one(2.0), 1, TS)
            .unwrap();
        assert!(matches!(
            store.apply_delta("binance", "BTC-USDT", &[], &bad_price, 2, TS),
            Err(Error::InvalidData(_))
        ));
        assert_eq!(store.snapshot("binance", "BTC-USDT", 0).unwrap().seq, 1);
    }
}
