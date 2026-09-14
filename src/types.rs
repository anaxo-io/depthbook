//! The book itself: levels, sides, and a full book.

use crate::decimal::Scale9;
use crate::intern::InternedString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Maximum number of levels held on one side of a book.
pub const MAX_LEVELS: usize = 200;

/// A single price level: a price and the quantity resting at it.
///
/// Both fields are [`Scale9`] fixed-point values, so there is no floating-point error and
/// an unscaled number cannot be passed by mistake.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Level {
    /// Price of this level.
    pub price: Scale9,
    /// Quantity resting at this price. Zero means the level should be removed.
    pub qty: Scale9,
}

impl Level {
    /// Create a level.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level};
    ///
    /// let level = Level::new(f64_to_scale9(50_000.5), f64_to_scale9(1.5));
    /// assert_eq!(level.price, f64_to_scale9(50_000.5));
    /// assert_eq!(level.qty, f64_to_scale9(1.5));
    /// ```
    #[inline]
    pub const fn new(price: Scale9, qty: Scale9) -> Self {
        Self { price, qty }
    }

    /// Whether this level has zero quantity, meaning it should be removed.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.qty.is_zero()
    }
}

/// One side of a book: a fixed-capacity array of levels kept in price order.
///
/// Bids are ordered highest price first, asks lowest first, so the best level is always
/// at index 0. The array is pre-allocated, so applying an update never allocates.
///
/// Capacity is [`MAX_LEVELS`]. Inserting into a full side drops the worst level.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Side {
    levels: [Level; MAX_LEVELS],
    count: usize,
}

impl Side {
    /// Create an empty side.
    ///
    /// # Examples
    /// ```
    /// use orderbook::Side;
    ///
    /// let side = Side::new();
    /// assert_eq!(side.count(), 0);
    /// assert!(side.is_empty());
    /// ```
    pub const fn new() -> Self {
        Self {
            levels: [Level {
                price: Scale9::ZERO,
                qty: Scale9::ZERO,
            }; MAX_LEVELS],
            count: 0,
        }
    }

    /// Number of levels currently held.
    #[inline]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// Whether this side holds no levels.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The levels currently held, best first.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level, Side};
    ///
    /// let mut side = Side::new();
    /// side.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), false);
    /// assert_eq!(side.levels().len(), 1);
    /// ```
    #[inline]
    pub fn levels(&self) -> &[Level] {
        &self.levels[..self.count]
    }

    /// Insert a level, or update the quantity if that price is already present.
    ///
    /// A level with zero quantity removes that price. `is_bid` selects the sort order:
    /// descending for bids, ascending for asks.
    ///
    /// Returns `true` if a new level was inserted, `false` if an existing one was updated,
    /// removed, or the side was full and the level was dropped.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level, Side};
    ///
    /// let mut bids = Side::new();
    /// let level = Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0));
    /// assert!(bids.insert(level, true));
    /// assert_eq!(bids.count(), 1);
    ///
    /// let updated = Level::new(f64_to_scale9(50_000.0), f64_to_scale9(2.0));
    /// assert!(!bids.insert(updated, true));
    /// assert_eq!(bids.count(), 1);
    /// assert_eq!(bids.best().unwrap().qty, f64_to_scale9(2.0));
    /// ```
    pub fn insert(&mut self, level: Level, is_bid: bool) -> bool {
        let insert_pos = self.find_insert_position(level.price, is_bid);

        if insert_pos < self.count && self.levels[insert_pos].price == level.price {
            if level.is_empty() {
                self.remove_at(insert_pos);
            } else {
                self.levels[insert_pos] = level;
            }
            return false;
        }

        if level.is_empty() {
            return false;
        }

        if self.count >= MAX_LEVELS {
            if insert_pos >= MAX_LEVELS {
                return false;
            }
            self.count = MAX_LEVELS - 1;
        }

        if insert_pos < self.count {
            self.levels
                .copy_within(insert_pos..self.count, insert_pos + 1);
        }

        self.levels[insert_pos] = level;
        self.count += 1;

        true
    }

    /// Remove the level at `price`, returning whether it was found.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level, Side};
    ///
    /// let mut bids = Side::new();
    /// bids.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), true);
    ///
    /// assert!(bids.remove(f64_to_scale9(50_000.0), true));
    /// assert_eq!(bids.count(), 0);
    /// assert!(!bids.remove(f64_to_scale9(50_000.0), true));
    /// ```
    pub fn remove(&mut self, price: Scale9, is_bid: bool) -> bool {
        let pos = self.find_insert_position(price, is_bid);

        if pos < self.count && self.levels[pos].price == price {
            self.remove_at(pos);
            true
        } else {
            false
        }
    }

    /// Set the quantity at `price`, inserting or removing the level as needed.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Side};
    ///
    /// let mut bids = Side::new();
    /// bids.update(f64_to_scale9(50_000.0), f64_to_scale9(1.0), true);
    /// assert_eq!(bids.count(), 1);
    ///
    /// bids.update(f64_to_scale9(50_000.0), f64_to_scale9(2.0), true);
    /// assert_eq!(bids.best().unwrap().qty, f64_to_scale9(2.0));
    ///
    /// bids.update(f64_to_scale9(50_000.0), f64_to_scale9(0.0), true);
    /// assert_eq!(bids.count(), 0);
    /// ```
    pub fn update(&mut self, price: Scale9, qty: Scale9, is_bid: bool) {
        self.insert(Level::new(price, qty), is_bid);
    }

    /// The best level: highest price for bids, lowest for asks.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level, Side};
    ///
    /// let mut bids = Side::new();
    /// assert!(bids.best().is_none());
    ///
    /// bids.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), true);
    /// bids.insert(Level::new(f64_to_scale9(49_999.0), f64_to_scale9(2.0)), true);
    ///
    /// assert_eq!(bids.best().unwrap().price, f64_to_scale9(50_000.0));
    /// ```
    #[inline]
    pub fn best(&self) -> Option<Level> {
        if self.count > 0 {
            Some(self.levels[0])
        } else {
            None
        }
    }

    /// Total quantity resting within `bps` basis points of the best price.
    ///
    /// One basis point is 0.01%. Returns zero if the side is empty.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level, Side};
    ///
    /// let mut bids = Side::new();
    /// bids.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), true);
    /// bids.insert(Level::new(f64_to_scale9(49_000.0), f64_to_scale9(2.0)), true);
    ///
    /// assert_eq!(bids.depth_within_bps(2500, true), f64_to_scale9(3.0));
    /// assert_eq!(bids.depth_within_bps(50, true), f64_to_scale9(1.0));
    /// ```
    pub fn depth_within_bps(&self, bps: u32, is_bid: bool) -> Scale9 {
        if self.count == 0 {
            return Scale9::ZERO;
        }

        let best = self.levels[0].price.raw();
        if best == 0 {
            return Scale9::ZERO;
        }

        let margin = best / 10_000 * bps as i64;
        let threshold = if is_bid { best - margin } else { best + margin };

        let mut total = Scale9::ZERO;
        for level in self.levels() {
            let within = if is_bid {
                level.price.raw() >= threshold
            } else {
                level.price.raw() <= threshold
            };

            if !within {
                // Levels are sorted, so everything further out is also outside the range.
                break;
            }
            total = total.saturating_add(level.qty);
        }

        total
    }

    /// Remove every level.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// Keep at most `n` levels, dropping the worst ones.
    ///
    /// Because a side is sorted best-first, this keeps the top of the book. Truncating to
    /// more levels than are present does nothing.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Level, Side};
    ///
    /// let mut bids = Side::new();
    /// bids.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), true);
    /// bids.insert(Level::new(f64_to_scale9(49_999.0), f64_to_scale9(2.0)), true);
    ///
    /// bids.truncate(1);
    /// assert_eq!(bids.count(), 1);
    /// assert_eq!(bids.best().unwrap().price, f64_to_scale9(50_000.0));
    /// ```
    pub fn truncate(&mut self, n: usize) {
        self.count = self.count.min(n);
    }

    /// Binary search for where `price` belongs, respecting the side's sort order.
    #[inline]
    fn find_insert_position(&self, price: Scale9, is_bid: bool) -> usize {
        self.levels()
            .binary_search_by(|level| {
                if is_bid {
                    level.price.cmp(&price).reverse()
                } else {
                    level.price.cmp(&price)
                }
            })
            .unwrap_or_else(|pos| pos)
    }

    #[inline]
    fn remove_at(&mut self, pos: usize) {
        if pos < self.count {
            self.levels.copy_within(pos + 1..self.count, pos);
            self.count -= 1;
        }
    }
}

impl Default for Side {
    fn default() -> Self {
        Self::new()
    }
}

// Serialize only the active levels; the rest of the array is padding.
impl Serialize for Side {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Side", 2)?;
        state.serialize_field("levels", &self.levels())?;
        state.serialize_field("count", &self.count)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Side {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Levels,
            Count,
        }

        struct SideVisitor;

        impl<'de> Visitor<'de> for SideVisitor {
            type Value = Side;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Side")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Side, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut levels: Option<Vec<Level>> = None;
                let mut count: Option<usize> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Levels => {
                            if levels.is_some() {
                                return Err(de::Error::duplicate_field("levels"));
                            }
                            levels = Some(map.next_value()?);
                        }
                        Field::Count => {
                            if count.is_some() {
                                return Err(de::Error::duplicate_field("count"));
                            }
                            count = Some(map.next_value()?);
                        }
                    }
                }

                let levels_vec = levels.ok_or_else(|| de::Error::missing_field("levels"))?;
                let count = count.ok_or_else(|| de::Error::missing_field("count"))?;

                let mut side = Side::new();
                for (i, level) in levels_vec.iter().take(MAX_LEVELS).enumerate() {
                    side.levels[i] = *level;
                }
                side.count = count.min(MAX_LEVELS).min(levels_vec.len());

                Ok(side)
            }
        }

        deserializer.deserialize_struct("Side", &["levels", "count"], SideVisitor)
    }
}

/// A complete order book for one instrument at one venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Book {
    /// Venue identifier, for example `binance`.
    pub venue: InternedString,
    /// Instrument identifier, for example `BTC-USDT`.
    pub inst: InternedString,
    /// Exchange timestamp in nanoseconds since the Unix epoch.
    pub ts: u64,
    /// Sequence number of the last update applied.
    pub seq: u64,
    /// Bid side, highest price first.
    pub bids: Side,
    /// Ask side, lowest price first.
    pub asks: Side,
}

impl Book {
    /// Create an empty book.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{Book, InternedString};
    ///
    /// let book = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1_700_000_000_000_000_000,
    ///     42,
    /// );
    /// assert_eq!(book.venue.as_str(), "binance");
    /// ```
    pub const fn new(venue: InternedString, inst: InternedString, ts: u64, seq: u64) -> Self {
        Self {
            venue,
            inst,
            ts,
            seq,
            bids: Side::new(),
            asks: Side::new(),
        }
    }

    /// Midpoint between the best bid and best ask.
    ///
    /// Returns `None` if either side is empty. A crossed book yields a midpoint outside
    /// both sides rather than an error; see issue #1.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Book, InternedString, Level};
    ///
    /// let mut book = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1_700_000_000_000_000_000,
    ///     42,
    /// );
    /// book.bids.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), true);
    /// book.asks.insert(Level::new(f64_to_scale9(50_010.0), f64_to_scale9(1.0)), false);
    ///
    /// assert_eq!(book.mid_price(), Some(f64_to_scale9(50_005.0)));
    /// ```
    pub fn mid_price(&self) -> Option<Scale9> {
        let bid = self.bids.best()?.price;
        let ask = self.asks.best()?.price;
        Some(Scale9::from_raw((bid.raw() + ask.raw()) / 2))
    }

    /// Difference between the best ask and the best bid.
    ///
    /// Returns `None` if either side is empty. A crossed book yields a negative spread
    /// rather than an error; see issue #1.
    ///
    /// # Examples
    /// ```
    /// use orderbook::{f64_to_scale9, Book, InternedString, Level};
    ///
    /// let mut book = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1_700_000_000_000_000_000,
    ///     42,
    /// );
    /// book.bids.insert(Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)), true);
    /// book.asks.insert(Level::new(f64_to_scale9(50_010.0), f64_to_scale9(1.0)), false);
    ///
    /// assert_eq!(book.spread(), Some(f64_to_scale9(10.0)));
    /// ```
    pub fn spread(&self) -> Option<Scale9> {
        let bid = self.bids.best()?.price;
        let ask = self.asks.best()?.price;
        Some(ask - bid)
    }

    /// Serialise to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialise to indented JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::f64_to_scale9;

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
    fn level_reports_empty_on_zero_qty() {
        let level = Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.5));
        assert_eq!(level.price, f64_to_scale9(50_000.0));
        assert!(!level.is_empty());

        assert!(Level::new(f64_to_scale9(50_000.0), Scale9::ZERO).is_empty());
    }

    #[test]
    fn new_side_is_empty() {
        let side = Side::new();
        assert_eq!(side.count(), 0);
        assert!(side.is_empty());
        assert!(side.best().is_none());
    }

    #[test]
    fn bids_sort_descending() {
        let mut bids = Side::new();

        assert!(bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true
        ));
        assert!(bids.insert(
            Level::new(f64_to_scale9(49_999.0), f64_to_scale9(2.0)),
            true
        ));
        assert!(bids.insert(
            Level::new(f64_to_scale9(50_001.0), f64_to_scale9(0.5)),
            true
        ));

        assert_eq!(bids.count(), 3);
        assert_eq!(bids.levels()[0].price, f64_to_scale9(50_001.0));
        assert_eq!(bids.levels()[1].price, f64_to_scale9(50_000.0));
        assert_eq!(bids.levels()[2].price, f64_to_scale9(49_999.0));
    }

    #[test]
    fn asks_sort_ascending() {
        let mut asks = Side::new();

        assert!(asks.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            false
        ));
        assert!(asks.insert(
            Level::new(f64_to_scale9(50_001.0), f64_to_scale9(2.0)),
            false
        ));
        assert!(asks.insert(
            Level::new(f64_to_scale9(49_999.0), f64_to_scale9(0.5)),
            false
        ));

        assert_eq!(asks.count(), 3);
        assert_eq!(asks.levels()[0].price, f64_to_scale9(49_999.0));
        assert_eq!(asks.levels()[1].price, f64_to_scale9(50_000.0));
        assert_eq!(asks.levels()[2].price, f64_to_scale9(50_001.0));
    }

    #[test]
    fn inserting_a_known_price_updates_it() {
        let mut bids = Side::new();

        bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        assert_eq!(bids.count(), 1);

        assert!(!bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(2.0)),
            true
        ));
        assert_eq!(bids.count(), 1);
        assert_eq!(bids.levels()[0].qty, f64_to_scale9(2.0));
    }

    #[test]
    fn remove_takes_a_level_out() {
        let mut bids = Side::new();

        bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        bids.insert(
            Level::new(f64_to_scale9(49_999.0), f64_to_scale9(2.0)),
            true,
        );
        assert_eq!(bids.count(), 2);

        assert!(bids.remove(f64_to_scale9(50_000.0), true));
        assert_eq!(bids.count(), 1);
        assert_eq!(bids.levels()[0].price, f64_to_scale9(49_999.0));

        assert!(!bids.remove(f64_to_scale9(50_000.0), true));
    }

    #[test]
    fn update_inserts_then_removes() {
        let mut bids = Side::new();

        bids.update(f64_to_scale9(50_000.0), f64_to_scale9(1.0), true);
        assert_eq!(bids.count(), 1);

        bids.update(f64_to_scale9(50_000.0), f64_to_scale9(2.0), true);
        assert_eq!(bids.count(), 1);
        assert_eq!(bids.levels()[0].qty, f64_to_scale9(2.0));

        bids.update(f64_to_scale9(50_000.0), Scale9::ZERO, true);
        assert_eq!(bids.count(), 0);
    }

    #[test]
    fn best_is_the_top_of_book() {
        let mut bids = Side::new();

        bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        bids.insert(
            Level::new(f64_to_scale9(49_999.0), f64_to_scale9(2.0)),
            true,
        );

        let best = bids.best().unwrap();
        assert_eq!(best.price, f64_to_scale9(50_000.0));
        assert_eq!(best.qty, f64_to_scale9(1.0));
    }

    #[test]
    fn depth_within_bps_sums_nearby_levels() {
        let mut bids = Side::new();

        bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        bids.insert(
            Level::new(f64_to_scale9(49_000.0), f64_to_scale9(2.0)),
            true,
        );
        bids.insert(
            Level::new(f64_to_scale9(40_000.0), f64_to_scale9(3.0)),
            true,
        );

        assert_eq!(bids.depth_within_bps(3000, true), f64_to_scale9(6.0));
        assert_eq!(bids.depth_within_bps(50, true), f64_to_scale9(1.0));
        assert_eq!(bids.depth_within_bps(1900, true), f64_to_scale9(3.0));
    }

    #[test]
    fn depth_within_bps_on_empty_side_is_zero() {
        assert_eq!(Side::new().depth_within_bps(100, true), Scale9::ZERO);
    }

    #[test]
    fn book_reports_mid_and_spread() {
        let mut snapshot = book();

        snapshot.bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        snapshot.asks.insert(
            Level::new(f64_to_scale9(50_010.0), f64_to_scale9(1.0)),
            false,
        );

        assert_eq!(snapshot.mid_price(), Some(f64_to_scale9(50_005.0)));
        assert_eq!(snapshot.spread(), Some(f64_to_scale9(10.0)));
    }

    #[test]
    fn mid_and_spread_need_both_sides() {
        let mut snapshot = book();
        assert_eq!(snapshot.mid_price(), None);

        snapshot.bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        assert_eq!(snapshot.mid_price(), None);
        assert_eq!(snapshot.spread(), None);
    }

    #[test]
    fn book_round_trips_through_json() {
        let mut snapshot = book();

        snapshot.bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        snapshot.asks.insert(
            Level::new(f64_to_scale9(50_010.0), f64_to_scale9(1.0)),
            false,
        );

        let json = snapshot.to_json().unwrap();
        assert!(json.contains("binance"));
        assert!(json.contains("BTC-USDT"));

        let back: Book = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bids.count(), 1);
        assert_eq!(back.bids.best().unwrap().price, f64_to_scale9(50_000.0));
        assert_eq!(back.asks.best().unwrap().price, f64_to_scale9(50_010.0));
    }

    #[test]
    fn clear_empties_a_side() {
        let mut bids = Side::new();
        bids.insert(
            Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0)),
            true,
        );
        bids.insert(
            Level::new(f64_to_scale9(49_999.0), f64_to_scale9(2.0)),
            true,
        );
        assert_eq!(bids.count(), 2);

        bids.clear();
        assert!(bids.is_empty());
    }

    #[test]
    fn a_full_side_drops_the_worst_level() {
        let mut bids = Side::new();

        for i in 0..MAX_LEVELS {
            let price = f64_to_scale9(50_000.0 - i as f64);
            bids.insert(Level::new(price, f64_to_scale9(1.0)), true);
        }
        assert_eq!(bids.count(), MAX_LEVELS);

        bids.insert(
            Level::new(f64_to_scale9(50_001.0), f64_to_scale9(1.0)),
            true,
        );
        assert_eq!(bids.count(), MAX_LEVELS);
        assert_eq!(bids.levels()[0].price, f64_to_scale9(50_001.0));
    }

    #[test]
    fn truncate_keeps_the_best_levels() {
        let mut bids = Side::new();
        for i in 0..10 {
            bids.insert(
                Level::new(f64_to_scale9(50_000.0 - i as f64), f64_to_scale9(1.0)),
                true,
            );
        }

        bids.truncate(3);
        assert_eq!(bids.count(), 3);
        assert_eq!(bids.best().unwrap().price, f64_to_scale9(50_000.0));

        bids.truncate(100);
        assert_eq!(bids.count(), 3);
    }
}
