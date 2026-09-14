use orderbook::types::{Level, Side};
use proptest::prelude::*;

// Property: Bids should always be in descending order
proptest! {
    #[test]
    fn test_bids_always_descending(prices in prop::collection::vec(0i64..1000000000000, 1..100)) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(price, 1_000000000), true);
        }

        // Check that all levels are in descending order
        let levels = bids.levels();
        for i in 1..levels.len() {
            prop_assert!(levels[i - 1].price >= levels[i].price,
                "Bids not in descending order: {} < {}", levels[i - 1].price, levels[i].price);
        }
    }
}

// Property: Asks should always be in ascending order
proptest! {
    #[test]
    fn test_asks_always_ascending(prices in prop::collection::vec(0i64..1000000000000, 1..100)) {
        let mut asks = Side::new();

        for price in prices {
            asks.insert(Level::new(price, 1_000000000), false);
        }

        // Check that all levels are in ascending order
        let levels = asks.levels();
        for i in 1..levels.len() {
            prop_assert!(levels[i - 1].price <= levels[i].price,
                "Asks not in ascending order: {} > {}", levels[i - 1].price, levels[i].price);
        }
    }
}

// Property: Inserting the same price twice should not increase count
proptest! {
    #[test]
    fn test_duplicate_price_updates(price in 0i64..1000000000000, qty1 in 1i64..10000000000, qty2 in 1i64..10000000000) {
        let mut bids = Side::new();

        bids.insert(Level::new(price, qty1), true);
        let count_after_first = bids.count();

        bids.insert(Level::new(price, qty2), true);
        let count_after_second = bids.count();

        prop_assert_eq!(count_after_first, count_after_second,
            "Duplicate price insertion changed count");

        // Check that the quantity was updated
        if let Some(best) = bids.best() {
            if best.price == price {
                prop_assert_eq!(best.qty, qty2, "Quantity not updated for duplicate price");
            }
        }
    }
}

// Property: Removing a price that exists should decrease count by 1
proptest! {
    #[test]
    fn test_remove_decreases_count(prices in prop::collection::vec(0i64..1000000000000, 2..50)) {
        let mut bids = Side::new();

        // Insert all prices
        for price in &prices {
            bids.insert(Level::new(*price, 1_000000000), true);
        }

        let initial_count = bids.count();

        // Remove the first unique price
        if let Some(&price_to_remove) = prices.first() {
            let removed = bids.remove(price_to_remove, true);
            if removed {
                prop_assert_eq!(bids.count(), initial_count - 1,
                    "Count not decreased after removal");
            }
        }
    }
}

// Property: Best bid should be the highest price
proptest! {
    #[test]
    fn test_best_bid_is_highest(prices in prop::collection::vec(1i64..1000000000000, 1..100)) {
        let mut bids = Side::new();

        for price in &prices {
            bids.insert(Level::new(*price, 1_000000000), true);
        }

        if let Some(best) = bids.best() {
            // Find the maximum price from all inserted prices
            let mut unique_prices: Vec<i64> = prices.clone();
            unique_prices.sort_unstable();
            unique_prices.dedup();

            if let Some(&max_price) = unique_prices.last() {
                prop_assert_eq!(best.price, max_price,
                    "Best bid is not the highest price");
            }
        }
    }
}

// Property: Best ask should be the lowest price
proptest! {
    #[test]
    fn test_best_ask_is_lowest(prices in prop::collection::vec(1i64..1000000000000, 1..100)) {
        let mut asks = Side::new();

        for price in &prices {
            asks.insert(Level::new(*price, 1_000000000), false);
        }

        if let Some(best) = asks.best() {
            // Find the minimum price from all inserted prices
            let mut unique_prices: Vec<i64> = prices.clone();
            unique_prices.sort_unstable();
            unique_prices.dedup();

            if let Some(&min_price) = unique_prices.first() {
                prop_assert_eq!(best.price, min_price,
                    "Best ask is not the lowest price");
            }
        }
    }
}

// Property: Inserting zero quantity should not add a level
proptest! {
    #[test]
    fn test_zero_quantity_not_inserted(price in 0i64..1000000000000) {
        let mut bids = Side::new();

        let inserted = bids.insert(Level::new(price, 0), true);

        prop_assert!(!inserted, "Zero quantity level was inserted");
        prop_assert_eq!(bids.count(), 0, "Count increased after inserting zero quantity");
    }
}

// Property: Updating to zero quantity should remove the level
proptest! {
    #[test]
    fn test_update_to_zero_removes(price in 0i64..1000000000000, initial_qty in 1i64..10000000000) {
        let mut bids = Side::new();

        bids.insert(Level::new(price, initial_qty), true);
        prop_assert_eq!(bids.count(), 1, "Initial insertion failed");

        bids.update(price, 0, true);
        prop_assert_eq!(bids.count(), 0, "Level not removed after update to zero");
    }
}

// Property: No duplicate prices should exist in the side
proptest! {
    #[test]
    fn test_no_duplicate_prices(prices in prop::collection::vec(0i64..1000000000000, 1..100)) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(price, 1_000000000), true);
        }

        let levels = bids.levels();
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                prop_assert_ne!(levels[i].price, levels[j].price,
                    "Duplicate price found at indices {} and {}", i, j);
            }
        }
    }
}

// Property: Count should never exceed 200
proptest! {
    #[test]
    fn test_count_never_exceeds_capacity(prices in prop::collection::vec(0i64..1000000000000, 1..300)) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(price, 1_000000000), true);
        }

        prop_assert!(bids.count() <= 200, "Count exceeded maximum capacity");
    }
}

// Property: depth_within_bps should be monotonically increasing with larger bps
proptest! {
    #[test]
    fn test_depth_monotonic(prices in prop::collection::vec(10000i64..20000i64, 10..50),
                            bps1 in 1u32..100,
                            bps2 in 100u32..1000) {
        let mut bids = Side::new();

        for price in prices {
            bids.insert(Level::new(price * 1_000000000, 1_000000000), true);
        }

        if bids.count() > 0 {
            let depth1 = bids.depth_within_bps(bps1, true);
            let depth2 = bids.depth_within_bps(bps2, true);

            prop_assert!(depth2 >= depth1,
                "Depth not monotonic: depth({}) = {} > depth({}) = {}",
                bps1, depth1, bps2, depth2);
        }
    }
}
