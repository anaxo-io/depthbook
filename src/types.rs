//! The book itself: levels, sides, and a full book.

use crate::intern::InternedString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Order book level with scale-9 integer representation.
///
/// All prices and quantities are stored as 64-bit integers with an implicit scale of 9.
/// For example:
/// - 123.456789 → 123_456_789_000
/// - 10.5 → 10_500_000_000
///
/// This representation ensures:
/// - No floating-point rounding errors
/// - Fast integer arithmetic
/// - Cache-friendly memory layout
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Level {
    /// Price in scale-9 representation (e.g., 123.456789 → 123_456_789_000)
    pub price: i64,
    /// Quantity in scale-9 representation (e.g., 10.5 → 10_500_000_000)
    pub qty: i64,
}

impl Level {
    /// Create a new order book level.
    ///
    /// # Arguments
    /// * `price` - Price in scale-9 representation
    /// * `qty` - Quantity in scale-9 representation
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::Level;
    ///
    /// // Price: 50000.123456789, Qty: 1.5
    /// let level = Level::new(50000_123456789, 1_500_000_000);
    /// assert_eq!(level.price, 50000_123456789);
    /// assert_eq!(level.qty, 1_500_000_000);
    /// ```
    #[inline]
    pub const fn new(price: i64, qty: i64) -> Self {
        Self { price, qty }
    }

    /// Check if this level has zero quantity (should be removed).
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.qty == 0
    }
}

/// One side (bids or asks) of an order book with fixed-size array.
///
/// Uses pre-allocated array to avoid heap allocations in hot path.
/// Maintains sorted order: bids descending by price, asks ascending by price.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Side {
    /// Pre-allocated array of levels (max 200 levels)
    levels: [Level; 200],
    /// Number of active levels in the array
    count: usize,
}

impl Side {
    /// Create a new empty order book side.
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::Side;
    ///
    /// let side = Side::new();
    /// assert_eq!(side.count(), 0);
    /// assert!(side.is_empty());
    /// ```
    pub const fn new() -> Self {
        Self {
            levels: [Level { price: 0, qty: 0 }; 200],
            count: 0,
        }
    }

    /// Get the number of active levels.
    #[inline]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// Check if this side has no levels.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get a slice of active levels.
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Side, Level};
    ///
    /// let mut side = Side::new();
    /// side.insert(Level::new(50000_000000000, 1_000000000), false);
    /// assert_eq!(side.levels().len(), 1);
    /// ```
    #[inline]
    pub fn levels(&self) -> &[Level] {
        &self.levels[..self.count]
    }

    /// Insert or update a level maintaining sorted order.
    ///
    /// # Arguments
    /// * `level` - The level to insert
    /// * `is_bid` - True for bids (descending), false for asks (ascending)
    ///
    /// # Returns
    /// `true` if a new level was inserted, `false` if an existing level was updated
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Side, Level};
    ///
    /// let mut bids = Side::new();
    /// let level = Level::new(50000_000000000, 1_000000000);
    /// assert!(bids.insert(level, true)); // New level inserted
    /// assert_eq!(bids.count(), 1);
    ///
    /// // Update existing level
    /// let updated = Level::new(50000_000000000, 2_000000000);
    /// assert!(!bids.insert(updated, true)); // Existing level updated
    /// assert_eq!(bids.count(), 1);
    /// assert_eq!(bids.best().unwrap().qty, 2_000000000);
    /// ```
    pub fn insert(&mut self, level: Level, is_bid: bool) -> bool {
        // Find the insertion point using binary search
        let insert_pos = self.find_insert_position(level.price, is_bid);

        // Check if we're updating an existing level
        if insert_pos < self.count && self.levels[insert_pos].price == level.price {
            if level.is_empty() {
                // Remove the level if quantity is zero
                self.remove_at(insert_pos);
                return false;
            } else {
                // Update existing level
                self.levels[insert_pos] = level;
                return false;
            }
        }

        // Don't insert empty levels
        if level.is_empty() {
            return false;
        }

        // Check capacity
        if self.count >= 200 {
            // If we're at capacity and inserting beyond the last position, ignore it
            if insert_pos >= 200 {
                return false;
            }
            // Otherwise, we'll drop the last level
            self.count = 199;
        }

        // Shift elements to make room
        if insert_pos < self.count {
            self.levels
                .copy_within(insert_pos..self.count, insert_pos + 1);
        }

        // Insert the new level
        self.levels[insert_pos] = level;
        self.count += 1;

        true
    }

    /// Remove a level at the given price.
    ///
    /// # Arguments
    /// * `price` - The price of the level to remove
    /// * `is_bid` - True for bids, false for asks
    ///
    /// # Returns
    /// `true` if the level was found and removed, `false` otherwise
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Side, Level};
    ///
    /// let mut bids = Side::new();
    /// let level = Level::new(50000_000000000, 1_000000000);
    /// bids.insert(level, true);
    ///
    /// assert!(bids.remove(50000_000000000, true));
    /// assert_eq!(bids.count(), 0);
    /// assert!(!bids.remove(50000_000000000, true)); // Already removed
    /// ```
    pub fn remove(&mut self, price: i64, is_bid: bool) -> bool {
        let pos = self.find_insert_position(price, is_bid);

        if pos < self.count && self.levels[pos].price == price {
            self.remove_at(pos);
            true
        } else {
            false
        }
    }

    /// Update the quantity of a level at the given price.
    ///
    /// If the quantity is zero, the level is removed.
    /// If the level doesn't exist and quantity is non-zero, it's inserted.
    ///
    /// # Arguments
    /// * `price` - The price of the level to update
    /// * `qty` - The new quantity (scale-9)
    /// * `is_bid` - True for bids, false for asks
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::Side;
    ///
    /// let mut bids = Side::new();
    /// bids.update(50000_000000000, 1_000000000, true); // Insert
    /// assert_eq!(bids.count(), 1);
    ///
    /// bids.update(50000_000000000, 2_000000000, true); // Update
    /// assert_eq!(bids.best().unwrap().qty, 2_000000000);
    ///
    /// bids.update(50000_000000000, 0, true); // Remove
    /// assert_eq!(bids.count(), 0);
    /// ```
    pub fn update(&mut self, price: i64, qty: i64, is_bid: bool) {
        let level = Level::new(price, qty);
        self.insert(level, is_bid);
    }

    /// Get the best level (first level).
    ///
    /// For bids, this is the highest price.
    /// For asks, this is the lowest price.
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Side, Level};
    ///
    /// let mut bids = Side::new();
    /// assert!(bids.best().is_none());
    ///
    /// bids.insert(Level::new(50000_000000000, 1_000000000), true);
    /// bids.insert(Level::new(49999_000000000, 2_000000000), true);
    ///
    /// let best = bids.best().unwrap();
    /// assert_eq!(best.price, 50000_000000000); // Highest price for bids
    /// ```
    #[inline]
    pub fn best(&self) -> Option<Level> {
        if self.count > 0 {
            Some(self.levels[0])
        } else {
            None
        }
    }

    /// Calculate total liquidity within N basis points of the best price.
    ///
    /// # Arguments
    /// * `bps` - Basis points (1 bps = 0.01% = 0.0001)
    /// * `is_bid` - True for bids, false for asks
    ///
    /// # Returns
    /// Total quantity (scale-9) within the specified basis points, or 0 if no levels exist
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Side, Level};
    ///
    /// let mut bids = Side::new();
    /// // Best bid at 50000
    /// bids.insert(Level::new(50000_000000000, 1_000000000), true);
    /// // Bid at 49000 (2000 bps away: ~20%)
    /// bids.insert(Level::new(49000_000000000, 2_000000000), true);
    ///
    /// // Within 2500 bps should include both levels
    /// let depth = bids.depth_within_bps(2500, true);
    /// assert_eq!(depth, 3_000000000);
    ///
    /// // Within 50 bps should only include the best level
    /// let depth = bids.depth_within_bps(50, true);
    /// assert_eq!(depth, 1_000000000);
    /// ```
    pub fn depth_within_bps(&self, bps: u32, is_bid: bool) -> i64 {
        if self.count == 0 {
            return 0;
        }

        let best_price = self.levels[0].price;
        if best_price == 0 {
            return 0;
        }

        // Calculate price threshold based on basis points
        // For bids: threshold = best_price * (1 - bps/10000)
        // For asks: threshold = best_price * (1 + bps/10000)
        let bps_i64 = bps as i64;
        let threshold = if is_bid {
            // For bids, we want prices >= best_price * (1 - bps/10000)
            // Using scale-9: best_price - (best_price * bps / 10000)
            best_price - (best_price * bps_i64 / 10000)
        } else {
            // For asks, we want prices <= best_price * (1 + bps/10000)
            best_price + (best_price * bps_i64 / 10000)
        };

        let mut total_qty = 0i64;
        for level in self.levels().iter() {
            let within_range = if is_bid {
                level.price >= threshold
            } else {
                level.price <= threshold
            };

            if within_range {
                total_qty = total_qty.saturating_add(level.qty);
            } else {
                // Since levels are sorted, we can stop early
                break;
            }
        }

        total_qty
    }

    /// Clear all levels from this side.
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
    /// use orderbook::types::{Side, Level};
    ///
    /// let mut bids = Side::new();
    /// bids.insert(Level::new(50000_000000000, 1_000000000), true);
    /// bids.insert(Level::new(49999_000000000, 2_000000000), true);
    ///
    /// bids.truncate(1);
    /// assert_eq!(bids.count(), 1);
    /// assert_eq!(bids.best().unwrap().price, 50000_000000000);
    /// ```
    pub fn truncate(&mut self, n: usize) {
        self.count = self.count.min(n);
    }

    /// Find the insertion position for a given price using binary search.
    ///
    /// For bids (descending): larger prices come first
    /// For asks (ascending): smaller prices come first
    #[inline]
    fn find_insert_position(&self, price: i64, is_bid: bool) -> usize {
        let levels = self.levels();

        levels
            .binary_search_by(|level| {
                if is_bid {
                    // Bids: descending order (higher prices first)
                    level.price.cmp(&price).reverse()
                } else {
                    // Asks: ascending order (lower prices first)
                    level.price.cmp(&price)
                }
            })
            .unwrap_or_else(|pos| pos)
    }

    /// Remove the level at the given position.
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

// Custom Serialize implementation that only serializes active levels
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

// Custom Deserialize implementation
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

        struct OrderBookSideVisitor;

        impl<'de> Visitor<'de> for OrderBookSideVisitor {
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

                // Create a new Side and copy levels
                let mut side = Side::new();
                for (i, level) in levels_vec
                    .iter()
                    .take(200.min(levels_vec.len()))
                    .enumerate()
                {
                    side.levels[i] = *level;
                }
                side.count = count.min(200).min(levels_vec.len());

                Ok(side)
            }
        }

        deserializer.deserialize_struct("Side", &["levels", "count"], OrderBookSideVisitor)
    }
}

/// Complete order book snapshot with both sides.
///
/// This structure provides an atomic view of the order book at a specific point in time.
/// It includes venue and instrument identifiers, timestamp, and sequence number.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Book {
    /// Venue identifier (e.g., "binance", "coinbase")
    pub venue: InternedString,
    /// Instrument identifier (e.g., "BTC-USDT", "ETH-USD")
    pub inst: InternedString,
    /// Timestamp in nanoseconds since epoch
    pub ts: u64,
    /// Sequence number for ordering events
    pub seq: u64,
    /// Bid side of the order book
    pub bids: Side,
    /// Ask side of the order book
    pub asks: Side,
}

impl Book {
    /// Create a new empty order book snapshot.
    ///
    /// # Arguments
    /// * `venue` - Venue identifier
    /// * `inst` - Instrument identifier
    /// * `ts` - Timestamp in nanoseconds
    /// * `seq` - Sequence number
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::Book;
    /// use orderbook::intern::InternedString;
    ///
    /// let snapshot = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1234567890000000000,
    ///     42
    /// );
    /// assert_eq!(snapshot.venue.as_str(), "binance");
    /// assert_eq!(snapshot.inst.as_str(), "BTC-USDT");
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

    /// Get the mid price (average of best bid and best ask).
    ///
    /// Returns `None` if either side has no levels.
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Book, Level};
    /// use orderbook::intern::InternedString;
    ///
    /// let mut snapshot = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1234567890000000000,
    ///     42
    /// );
    ///
    /// snapshot.bids.insert(Level::new(50000_000000000, 1_000000000), true);
    /// snapshot.asks.insert(Level::new(50010_000000000, 1_000000000), false);
    ///
    /// let mid = snapshot.mid_price().unwrap();
    /// assert_eq!(mid, 50005_000000000); // (50000 + 50010) / 2
    /// ```
    pub fn mid_price(&self) -> Option<i64> {
        let best_bid = self.bids.best()?;
        let best_ask = self.asks.best()?;
        Some((best_bid.price + best_ask.price) / 2)
    }

    /// Get the spread (difference between best ask and best bid).
    ///
    /// Returns `None` if either side has no levels.
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Book, Level};
    /// use orderbook::intern::InternedString;
    ///
    /// let mut snapshot = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1234567890000000000,
    ///     42
    /// );
    ///
    /// snapshot.bids.insert(Level::new(50000_000000000, 1_000000000), true);
    /// snapshot.asks.insert(Level::new(50010_000000000, 1_000000000), false);
    ///
    /// let spread = snapshot.spread().unwrap();
    /// assert_eq!(spread, 10_000000000); // 50010 - 50000 = 10
    /// ```
    pub fn spread(&self) -> Option<i64> {
        let best_bid = self.bids.best()?;
        let best_ask = self.asks.best()?;
        Some(best_ask.price - best_bid.price)
    }

    /// Convert the snapshot to JSON for WebSocket messages.
    ///
    /// # Examples
    /// ```
    /// use orderbook::types::{Book, Level};
    /// use orderbook::intern::InternedString;
    ///
    /// let mut snapshot = Book::new(
    ///     InternedString::new("binance"),
    ///     InternedString::new("BTC-USDT"),
    ///     1234567890000000000,
    ///     42
    /// );
    ///
    /// snapshot.bids.insert(Level::new(50000_000000000, 1_000000000), true);
    /// snapshot.asks.insert(Level::new(50010_000000000, 1_000000000), false);
    ///
    /// let json = snapshot.to_json().unwrap();
    /// assert!(json.contains("binance"));
    /// assert!(json.contains("BTC-USDT"));
    /// ```
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Convert the snapshot to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_book_level_creation() {
        let level = Level::new(50000_123456789, 1_500_000_000);
        assert_eq!(level.price, 50000_123456789);
        assert_eq!(level.qty, 1_500_000_000);
        assert!(!level.is_empty());
    }

    #[test]
    fn test_order_book_level_empty() {
        let level = Level::new(50000_000000000, 0);
        assert!(level.is_empty());
    }

    #[test]
    fn test_order_book_side_new() {
        let side = Side::new();
        assert_eq!(side.count(), 0);
        assert!(side.is_empty());
        assert!(side.best().is_none());
    }

    #[test]
    fn test_order_book_side_insert_bids() {
        let mut bids = Side::new();

        // Insert levels in random order
        assert!(bids.insert(Level::new(50000_000000000, 1_000000000), true));
        assert!(bids.insert(Level::new(49999_000000000, 2_000000000), true));
        assert!(bids.insert(Level::new(50001_000000000, 500000000), true));

        // Should be sorted descending
        assert_eq!(bids.count(), 3);
        assert_eq!(bids.levels()[0].price, 50001_000000000);
        assert_eq!(bids.levels()[1].price, 50000_000000000);
        assert_eq!(bids.levels()[2].price, 49999_000000000);
    }

    #[test]
    fn test_order_book_side_insert_asks() {
        let mut asks = Side::new();

        // Insert levels in random order
        assert!(asks.insert(Level::new(50000_000000000, 1_000000000), false));
        assert!(asks.insert(Level::new(50001_000000000, 2_000000000), false));
        assert!(asks.insert(Level::new(49999_000000000, 500000000), false));

        // Should be sorted ascending
        assert_eq!(asks.count(), 3);
        assert_eq!(asks.levels()[0].price, 49999_000000000);
        assert_eq!(asks.levels()[1].price, 50000_000000000);
        assert_eq!(asks.levels()[2].price, 50001_000000000);
    }

    #[test]
    fn test_order_book_side_update_existing() {
        let mut bids = Side::new();

        bids.insert(Level::new(50000_000000000, 1_000000000), true);
        assert_eq!(bids.count(), 1);

        // Update with same price, different quantity
        assert!(!bids.insert(Level::new(50000_000000000, 2_000000000), true));
        assert_eq!(bids.count(), 1);
        assert_eq!(bids.levels()[0].qty, 2_000000000);
    }

    #[test]
    fn test_order_book_side_remove() {
        let mut bids = Side::new();

        bids.insert(Level::new(50000_000000000, 1_000000000), true);
        bids.insert(Level::new(49999_000000000, 2_000000000), true);
        assert_eq!(bids.count(), 2);

        assert!(bids.remove(50000_000000000, true));
        assert_eq!(bids.count(), 1);
        assert_eq!(bids.levels()[0].price, 49999_000000000);

        assert!(!bids.remove(50000_000000000, true)); // Already removed
    }

    #[test]
    fn test_order_book_side_update() {
        let mut bids = Side::new();

        // Insert new level
        bids.update(50000_000000000, 1_000000000, true);
        assert_eq!(bids.count(), 1);

        // Update existing level
        bids.update(50000_000000000, 2_000000000, true);
        assert_eq!(bids.count(), 1);
        assert_eq!(bids.levels()[0].qty, 2_000000000);

        // Remove level with zero quantity
        bids.update(50000_000000000, 0, true);
        assert_eq!(bids.count(), 0);
    }

    #[test]
    fn test_order_book_side_best() {
        let mut bids = Side::new();

        bids.insert(Level::new(50000_000000000, 1_000000000), true);
        bids.insert(Level::new(49999_000000000, 2_000000000), true);

        let best = bids.best().unwrap();
        assert_eq!(best.price, 50000_000000000);
        assert_eq!(best.qty, 1_000000000);
    }

    #[test]
    fn test_order_book_side_depth_within_bps() {
        let mut bids = Side::new();

        // Best bid at 50000
        bids.insert(Level::new(50000_000000000, 1_000000000), true);
        // Bid at 49000 (2000 bps = 20% away: 50000 * 0.98 = 49000)
        bids.insert(Level::new(49000_000000000, 2_000000000), true);
        // Bid at 45000 (10000 bps away)
        bids.insert(Level::new(40000_000000000, 3_000000000), true);

        // Within 3000 bps: threshold = 50000 * (1 - 0.30) = 35000
        // All three levels should be included
        let depth = bids.depth_within_bps(3000, true);
        assert_eq!(depth, 6_000000000);

        // Within 50 bps: threshold = 50000 * (1 - 0.005) = 49750
        // Only first level should be included (50000)
        let depth = bids.depth_within_bps(50, true);
        assert_eq!(depth, 1_000000000);

        // Within 2000 bps: threshold = 50000 * (1 - 0.20) = 40000
        // First two levels should be included (50000 and 49000, but not 40000)
        let depth = bids.depth_within_bps(1900, true);
        assert_eq!(depth, 3_000000000);
    }

    #[test]
    fn test_order_book_snapshot_creation() {
        let snapshot = Book::new(
            InternedString::new("binance"),
            InternedString::new("BTC-USDT"),
            1234567890000000000,
            42,
        );

        assert_eq!(snapshot.venue.as_str(), "binance");
        assert_eq!(snapshot.inst.as_str(), "BTC-USDT");
        assert_eq!(snapshot.ts, 1234567890000000000);
        assert_eq!(snapshot.seq, 42);
    }

    #[test]
    fn test_order_book_snapshot_mid_price() {
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

        let mid = snapshot.mid_price().unwrap();
        assert_eq!(mid, 50005_000000000);
    }

    #[test]
    fn test_order_book_snapshot_spread() {
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

        let spread = snapshot.spread().unwrap();
        assert_eq!(spread, 10_000000000);
    }

    #[test]
    fn test_order_book_snapshot_to_json() {
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

        let json = snapshot.to_json().unwrap();
        assert!(json.contains("binance"));
        assert!(json.contains("BTC-USDT"));
    }

    #[test]
    fn test_order_book_side_clear() {
        let mut bids = Side::new();
        bids.insert(Level::new(50000_000000000, 1_000000000), true);
        bids.insert(Level::new(49999_000000000, 2_000000000), true);
        assert_eq!(bids.count(), 2);

        bids.clear();
        assert_eq!(bids.count(), 0);
        assert!(bids.is_empty());
    }

    #[test]
    fn test_order_book_side_capacity() {
        let mut bids = Side::new();

        // Fill up to capacity
        for i in 0..200 {
            let price = (50000 - i) * 1_000000000;
            bids.insert(Level::new(price, 1_000000000), true);
        }
        assert_eq!(bids.count(), 200);

        // Try to insert beyond capacity (should drop the worst level)
        bids.insert(Level::new(50001_000000000, 1_000000000), true);
        assert_eq!(bids.count(), 200);
        assert_eq!(bids.levels()[0].price, 50001_000000000);
    }
}
