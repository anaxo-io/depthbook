//! Invariants that must hold for any sequence of level insertions.
//!
//! Prices are generated as raw scaled integers and wrapped with [`s9`], so the
//! strategies stay simple while the book still sees `Scale9` values.

use orderbook::types::{Level, Side};
use orderbook::{Scale9, MAX_LEVELS};
use proptest::prelude::*;

/// Wrap a raw scaled integer as a price or quantity.
fn s9(raw: i64) -> Scale9 {
    Scale9::from_raw(raw)
}

/// One unit of quantity, used wherever the quantity is irrelevant to the property.
const ONE: Scale9 = Scale9::ONE;

// Bids are ordered highest price first.
proptest! {
    #[test]
    fn bids_always_descending(prices in prop::collection::vec(0i64..1_000_000_000_000, 1..100)) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(s9(price), ONE), true);
        }

        let levels: Vec<Level> = bids.levels().copied().collect();
        for i in 1..levels.len() {
            prop_assert!(
                levels[i - 1].price >= levels[i].price,
                "bids not descending: {} < {}",
                levels[i - 1].price,
                levels[i].price
            );
        }
    }
}

// Asks are ordered lowest price first.
proptest! {
    #[test]
    fn asks_always_ascending(prices in prop::collection::vec(0i64..1_000_000_000_000, 1..100)) {
        let mut asks = Side::new();

        for price in prices {
            asks.insert(Level::new(s9(price), ONE), false);
        }

        let levels: Vec<Level> = asks.levels().copied().collect();
        for i in 1..levels.len() {
            prop_assert!(
                levels[i - 1].price <= levels[i].price,
                "asks not ascending: {} > {}",
                levels[i - 1].price,
                levels[i].price
            );
        }
    }
}

// Inserting a price that is already present updates it rather than adding a level.
proptest! {
    #[test]
    fn duplicate_price_updates_in_place(
        price in 0i64..1_000_000_000_000,
        qty1 in 1i64..10_000_000_000,
        qty2 in 1i64..10_000_000_000,
    ) {
        let mut bids = Side::new();

        bids.insert(Level::new(s9(price), s9(qty1)), true);
        let count_after_first = bids.count();

        bids.insert(Level::new(s9(price), s9(qty2)), true);
        let count_after_second = bids.count();

        prop_assert_eq!(
            count_after_first,
            count_after_second,
            "duplicate price changed the level count"
        );

        if let Some(best) = bids.best() {
            if best.price == s9(price) {
                prop_assert_eq!(best.qty, s9(qty2), "quantity not updated for duplicate price");
            }
        }
    }
}

// Removing a price that is present drops the count by exactly one.
proptest! {
    #[test]
    fn remove_decreases_count(prices in prop::collection::vec(0i64..1_000_000_000_000, 2..50)) {
        let mut bids = Side::new();

        for price in &prices {
            bids.insert(Level::new(s9(*price), ONE), true);
        }

        let initial_count = bids.count();

        if let Some(&price_to_remove) = prices.first() {
            if bids.remove(s9(price_to_remove), true) {
                prop_assert_eq!(
                    bids.count(),
                    initial_count - 1,
                    "count not decreased after removal"
                );
            }
        }
    }
}

// The best bid is the highest price inserted.
proptest! {
    #[test]
    fn best_bid_is_highest(prices in prop::collection::vec(1i64..1_000_000_000_000, 1..100)) {
        let mut bids = Side::new();

        for price in &prices {
            bids.insert(Level::new(s9(*price), ONE), true);
        }

        if let Some(best) = bids.best() {
            let mut unique = prices.clone();
            unique.sort_unstable();
            unique.dedup();

            if let Some(&max_price) = unique.last() {
                prop_assert_eq!(best.price, s9(max_price), "best bid is not the highest price");
            }
        }
    }
}

// The best ask is the lowest price inserted.
proptest! {
    #[test]
    fn best_ask_is_lowest(prices in prop::collection::vec(1i64..1_000_000_000_000, 1..100)) {
        let mut asks = Side::new();

        for price in &prices {
            asks.insert(Level::new(s9(*price), ONE), false);
        }

        if let Some(best) = asks.best() {
            let mut unique = prices.clone();
            unique.sort_unstable();
            unique.dedup();

            if let Some(&min_price) = unique.first() {
                prop_assert_eq!(best.price, s9(min_price), "best ask is not the lowest price");
            }
        }
    }
}

// A level with zero quantity is never added.
proptest! {
    #[test]
    fn zero_quantity_is_not_inserted(price in 0i64..1_000_000_000_000) {
        let mut bids = Side::new();

        let inserted = bids.insert(Level::new(s9(price), Scale9::ZERO), true);

        prop_assert!(!inserted, "zero-quantity level was inserted");
        prop_assert_eq!(bids.count(), 0, "count increased after a zero-quantity insert");
    }
}

// Updating a level to zero quantity removes it.
proptest! {
    #[test]
    fn update_to_zero_removes(
        price in 0i64..1_000_000_000_000,
        initial_qty in 1i64..10_000_000_000,
    ) {
        let mut bids = Side::new();

        bids.insert(Level::new(s9(price), s9(initial_qty)), true);
        prop_assert_eq!(bids.count(), 1, "initial insertion failed");

        bids.update(s9(price), Scale9::ZERO, true);
        prop_assert_eq!(bids.count(), 0, "level not removed after update to zero");
    }
}

// A side never holds the same price twice.
proptest! {
    #[test]
    fn no_duplicate_prices(prices in prop::collection::vec(0i64..1_000_000_000_000, 1..100)) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(s9(price), ONE), true);
        }

        let levels: Vec<Level> = bids.levels().copied().collect();
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                prop_assert_ne!(
                    levels[i].price,
                    levels[j].price,
                    "duplicate price at indices {} and {}",
                    i,
                    j
                );
            }
        }
    }
}

// A side never exceeds its capacity, however many levels are pushed at it.
proptest! {
    #[test]
    fn count_never_exceeds_capacity(prices in prop::collection::vec(0i64..1_000_000_000_000, 1..300)) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(s9(price), ONE), true);
        }

        prop_assert!(bids.count() <= MAX_LEVELS, "count exceeded capacity");
    }
}

// Widening the basis-point window can only include more liquidity, never less.
proptest! {
    #[test]
    fn depth_is_monotonic_in_bps(
        prices in prop::collection::vec(10_000i64..20_000i64, 10..50),
        bps1 in 1u32..100,
        bps2 in 100u32..1000,
    ) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(s9(price * 1_000_000_000), ONE), true);
        }

        if bids.count() > 0 {
            let depth1 = bids.depth_within_bps(bps1, true);
            let depth2 = bids.depth_within_bps(bps2, true);

            prop_assert!(
                depth2 >= depth1,
                "depth not monotonic: depth({}) = {} > depth({}) = {}",
                bps1,
                depth1,
                bps2,
                depth2
            );
        }
    }
}
