//! Lock and atomic bookkeeping around a single [`Book`].

use crate::error::{Error, Result};
use crate::types::{Book, Level};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

/// A single order book plus the metadata needed to read it without taking the lock.
///
/// The book itself is behind an `RwLock`. The sequence number and last-update timestamp
/// are duplicated into atomics so that gap checks and staleness checks — the two things
/// done on every update — never contend with a reader holding the book.
#[derive(Debug)]
pub struct BookState {
    book: RwLock<Book>,
    last_update_ns: AtomicU64,
    sequence: AtomicU64,
    gapped: AtomicBool,
}

impl BookState {
    /// Wrap an initial book.
    pub fn new(book: Book) -> Self {
        let ts = book.ts;
        let seq = book.seq;

        Self {
            book: RwLock::new(book),
            last_update_ns: AtomicU64::new(ts),
            sequence: AtomicU64::new(seq),
            gapped: AtomicBool::new(false),
        }
    }

    /// Copy the current book.
    ///
    /// Takes the read lock without blocking where possible, falling back to a blocking
    /// acquire while a writer holds the lock.
    pub fn snapshot(&self) -> Book {
        match self.book.try_read() {
            Ok(book) => book.clone(),
            Err(_) => self.read().clone(),
        }
    }

    /// Copy the best bid and ask.
    ///
    /// Returns `None` if either side is empty.
    pub fn bbo(&self) -> Option<(Level, Level)> {
        let book = match self.book.try_read() {
            Ok(book) => book,
            Err(_) => self.read(),
        };
        Some((book.bids.best()?, book.asks.best()?))
    }

    /// Replace the book wholesale, as when applying a snapshot, and clear the gapped flag.
    ///
    /// The check runs under the write lock, so a snapshot cannot roll back a delta that
    /// was applied concurrently. Returns [`Error::OutOfOrder`] and leaves the book
    /// untouched if the snapshot's sequence number is below the book's.
    ///
    /// `ts` is the exchange timestamp in nanoseconds since the Unix epoch.
    pub fn replace(&self, new_book: Book, ts: u64) -> Result<()> {
        let seq = new_book.seq;
        let mut book = self.write();
        if seq < book.seq {
            return Err(Error::OutOfOrder {
                current: book.seq,
                received: seq,
            });
        }
        *book = new_book;
        self.last_update_ns.store(ts, Ordering::Release);
        self.sequence.store(seq, Ordering::Release);
        self.gapped.store(false, Ordering::Release);
        Ok(())
    }

    /// Apply a delta if `seq` is the next sequence number.
    ///
    /// The check happens under the write lock, so two writers cannot both pass it.
    /// Returns `Ok(true)` if applied, `Ok(false)` if `seq` is already known, and
    /// [`Error::SequenceGap`] if `seq` skips ahead; in the latter two cases the levels are
    /// untouched. A gap marks the book as gapped until the next snapshot.
    ///
    /// `ts` is the exchange timestamp in nanoseconds since the Unix epoch. It is recorded
    /// as given: the store does not substitute its own clock, so staleness is measured
    /// against the venue's view of time rather than the local one.
    pub fn apply<F>(&self, f: F, seq: u64, ts: u64) -> Result<bool>
    where
        F: FnOnce(&mut Book),
    {
        let mut book = self.write();
        let expected = book.seq.saturating_add(1);
        if seq > expected {
            book.gapped = true;
            self.gapped.store(true, Ordering::Release);
            return Err(Error::SequenceGap {
                expected,
                received: seq,
            });
        }
        if seq <= book.seq {
            return Ok(false);
        }
        f(&mut book);

        book.seq = seq;
        book.ts = ts;

        self.last_update_ns.store(ts, Ordering::Release);
        self.sequence.store(seq, Ordering::Release);
        Ok(true)
    }

    /// Whether a delta was rejected for a sequence gap since the last snapshot.
    ///
    /// Read without taking the lock. A gapped book still serves its last good state.
    #[inline]
    pub fn is_gapped(&self) -> bool {
        self.gapped.load(Ordering::Acquire)
    }

    /// The book's sequence number, read without taking the lock.
    #[inline]
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    /// The last update timestamp in nanoseconds, read without taking the lock.
    #[inline]
    pub fn last_update_ns(&self) -> u64 {
        self.last_update_ns.load(Ordering::Acquire)
    }

    /// Report whether the last update is older than `max_age_ms`.
    ///
    /// Compares the venue timestamp against the local wall clock, so a venue whose clock
    /// runs behind will look stale. A book timestamped in the future is never stale.
    pub fn is_stale(&self, max_age_ms: u64) -> bool {
        let last_update = self.last_update_ns.load(Ordering::Acquire);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        now.saturating_sub(last_update) / 1_000_000 > max_age_ms
    }

    /// Take the read lock, recovering from a poisoned lock.
    ///
    /// A panic while holding the write lock can leave a book half-updated, but the
    /// alternative — propagating the panic to every reader — turns one bad update into a
    /// dead feed. The next snapshot overwrites whatever was left behind.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Book> {
        self.book.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Take the write lock, recovering from a poisoned lock. See [`BookState::read`].
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Book> {
        self.book.write().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::f64_to_scale9;
    use crate::intern::InternedString;

    const TS: u64 = 1_700_000_000_000_000_000;

    fn book() -> Book {
        Book::new(
            InternedString::new("binance"),
            InternedString::new("BTC-USDT"),
            TS,
            42,
        )
    }

    #[test]
    fn new_state_exposes_seq_and_ts() {
        let state = BookState::new(book());
        assert_eq!(state.sequence(), 42);
        assert_eq!(state.last_update_ns(), TS);
    }

    #[test]
    fn snapshot_returns_a_copy() {
        let mut b = book();
        b.bids
            .insert(Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)), true);

        let state = BookState::new(b);
        let copy = state.snapshot();

        assert_eq!(copy.bids.count(), 1);
        assert_eq!(copy.venue.as_str(), "binance");
    }

    #[test]
    fn bbo_needs_both_sides() {
        let mut b = book();
        b.bids
            .insert(Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)), true);

        let state = BookState::new(b);
        assert!(state.bbo().is_none());

        state
            .apply(
                |book| {
                    book.asks.insert(
                        Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0)),
                        false,
                    );
                },
                43,
                TS + 1,
            )
            .unwrap();

        let (bid, ask) = state.bbo().unwrap();
        assert_eq!(bid.price, f64_to_scale9(50000.0));
        assert_eq!(ask.price, f64_to_scale9(50001.0));
    }

    #[test]
    fn apply_records_the_given_timestamp() {
        let state = BookState::new(book());

        state
            .apply(
                |book| {
                    book.bids
                        .insert(Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)), true);
                },
                43,
                TS + 1000,
            )
            .unwrap();

        assert_eq!(state.sequence(), 43);
        assert_eq!(state.last_update_ns(), TS + 1000);
        assert_eq!(state.snapshot().ts, TS + 1000);
    }

    #[test]
    fn apply_rejects_gaps_and_ignores_duplicates_under_the_lock() {
        let state = BookState::new(book());
        let touch = |book: &mut Book| {
            book.bids
                .insert(Level::new(f64_to_scale9(1.0), f64_to_scale9(1.0)), true);
        };

        assert!(matches!(
            state.apply(touch, 44, TS),
            Err(Error::SequenceGap {
                expected: 43,
                received: 44
            })
        ));
        assert_eq!(state.apply(touch, 42, TS), Ok(false));
        assert_eq!(state.snapshot().bids.count(), 0);
        assert_eq!(state.apply(touch, 43, TS), Ok(true));
        assert_eq!(state.sequence(), 43);
    }

    #[test]
    fn replace_swaps_the_whole_book() {
        let state = BookState::new(book());
        let mut fresh = book();
        fresh.seq = 100;
        fresh
            .bids
            .insert(Level::new(f64_to_scale9(1.0), f64_to_scale9(1.0)), true);

        state.replace(fresh, TS + 5).unwrap();

        assert_eq!(state.sequence(), 100);
        assert_eq!(state.last_update_ns(), TS + 5);
        assert_eq!(state.snapshot().bids.count(), 1);
    }

    #[test]
    fn old_timestamp_is_stale() {
        let state = BookState::new(book());
        assert!(state.is_stale(1000));
    }
}
