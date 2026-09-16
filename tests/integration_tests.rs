use depthbook::decimal::{f64_to_scale9, scale9_to_f64, scale9_to_string};
use depthbook::intern::InternedString;
use depthbook::types::{Book, Level};

#[test]
fn test_realistic_order_book_workflow() {
    // Create a new order book for BTC-USDT on Binance
    let mut book = Book::new(
        InternedString::new("binance"),
        InternedString::new("BTC-USDT"),
        1234567890000000000,
        1,
    );

    // Add some realistic bids
    book.bids.insert(
        Level::new(f64_to_scale9(50000.12), f64_to_scale9(1.5)),
        true,
    );
    book.bids.insert(
        Level::new(f64_to_scale9(49999.50), f64_to_scale9(2.3)),
        true,
    );
    book.bids.insert(
        Level::new(f64_to_scale9(49998.00), f64_to_scale9(0.75)),
        true,
    );

    // Add some realistic asks
    book.asks.insert(
        Level::new(f64_to_scale9(50001.25), f64_to_scale9(1.0)),
        false,
    );
    book.asks.insert(
        Level::new(f64_to_scale9(50002.50), f64_to_scale9(1.8)),
        false,
    );
    book.asks.insert(
        Level::new(f64_to_scale9(50003.75), f64_to_scale9(2.5)),
        false,
    );

    // Verify best bid and ask
    let best_bid = book.bids.best().unwrap();
    let best_ask = book.asks.best().unwrap();

    assert!(scale9_to_f64(best_bid.price) > 50000.0);
    assert!(scale9_to_f64(best_ask.price) > 50001.0);

    // Calculate mid price
    let mid = book.mid_price().unwrap();
    assert!(scale9_to_f64(mid) > 50000.0);
    assert!(scale9_to_f64(mid) < 50002.0);

    // Calculate spread
    let spread = book.spread().unwrap();
    assert!(scale9_to_f64(spread) < 2.0);

    // Verify order book side counts
    assert_eq!(book.bids.count(), 3);
    assert_eq!(book.asks.count(), 3);
}

#[test]
fn test_order_book_updates() {
    let mut book = Book::new(
        InternedString::new("coinbase"),
        InternedString::new("ETH-USD"),
        1234567890000000000,
        1,
    );

    // Add initial levels
    for i in 0..10 {
        let bid_price = f64_to_scale9(3000.0 - i as f64);
        let ask_price = f64_to_scale9(3010.0 + i as f64);

        book.bids
            .insert(Level::new(bid_price, f64_to_scale9(1.0)), true);
        book.asks
            .insert(Level::new(ask_price, f64_to_scale9(1.0)), false);
    }

    assert_eq!(book.bids.count(), 10);
    assert_eq!(book.asks.count(), 10);

    // Update a level
    book.bids
        .update(f64_to_scale9(2995.0), f64_to_scale9(5.0), true);
    assert_eq!(book.bids.count(), 10); // Count shouldn't change

    // Remove a level by setting quantity to zero
    book.bids
        .update(f64_to_scale9(2995.0), f64_to_scale9(0.0), true);
    assert_eq!(book.bids.count(), 9); // Count should decrease

    // Add a new level
    book.bids
        .insert(Level::new(f64_to_scale9(2985.0), f64_to_scale9(2.0)), true);
    assert_eq!(book.bids.count(), 10);
}

#[test]
fn test_order_book_serialization() {
    let mut book = Book::new(
        InternedString::new("kraken"),
        InternedString::new("SOL-USD"),
        1234567890000000000,
        1,
    );

    // Add some levels
    book.bids
        .insert(Level::new(f64_to_scale9(100.50), f64_to_scale9(10.0)), true);
    book.asks.insert(
        Level::new(f64_to_scale9(100.75), f64_to_scale9(15.0)),
        false,
    );

    // Serialize to JSON
    let json = book.to_json().unwrap();
    assert!(json.contains("kraken"));
    assert!(json.contains("SOL-USD"));

    // Pretty print
    let pretty = book.to_json_pretty().unwrap();
    assert!(pretty.contains("kraken"));
    assert!(pretty.contains("  ")); // Should have indentation
}

#[test]
fn test_depth_calculation() {
    let mut bids = depthbook::types::Side::new();

    // Add levels at different price points
    bids.insert(Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)), true);
    bids.insert(Level::new(f64_to_scale9(49950.0), f64_to_scale9(2.0)), true);
    bids.insert(Level::new(f64_to_scale9(49900.0), f64_to_scale9(3.0)), true);
    bids.insert(Level::new(f64_to_scale9(49500.0), f64_to_scale9(4.0)), true);

    // Within 5 bps (~0.05%): should include first level only (threshold: 49975)
    let depth = bids.depth_within_bps(5, true);
    assert_eq!(scale9_to_f64(depth), 1.0);

    // Within 80 bps (~0.8%): should include first three levels (50000, 49950, 49900)
    let depth = bids.depth_within_bps(80, true);
    assert_eq!(scale9_to_f64(depth), 6.0);

    // Within 1000 bps (~10%): should include all levels
    let depth = bids.depth_within_bps(1000, true);
    assert_eq!(scale9_to_f64(depth), 10.0);
}

#[test]
fn test_string_interning_efficiency() {
    // Create multiple snapshots with the same venue/instrument
    let venue = InternedString::new("binance");
    let inst = InternedString::new("BTC-USDT");

    let snapshot1 = Book::new(venue.clone(), inst.clone(), 1234567890000000000, 1);

    let snapshot2 = Book::new(
        InternedString::new("binance"),
        InternedString::new("BTC-USDT"),
        1234567890000000001,
        2,
    );

    // Both should use the same interned strings
    assert_eq!(snapshot1.venue, snapshot2.venue);
    assert_eq!(snapshot1.inst, snapshot2.inst);
}

#[test]
fn test_decimal_conversion_precision() {
    // Test that we maintain precision through conversions
    let prices = [123.456789012, 0.000000001, 999999.999999999, 50000.12345];

    for &price in &prices {
        let scale9 = f64_to_scale9(price);
        let back = scale9_to_f64(scale9);

        // Should be very close (within floating point precision)
        assert!(
            (back - price).abs() < 1e-8,
            "Price conversion failed: {} -> {} -> {}",
            price,
            scale9,
            back
        );
    }
}

#[test]
fn test_order_book_side_sorting() {
    let mut bids = depthbook::types::Side::new();

    // Insert in random order
    let prices = [49950.0, 50000.0, 49900.0, 49999.0, 49800.0];
    for price in prices {
        bids.insert(Level::new(f64_to_scale9(price), f64_to_scale9(1.0)), true);
    }

    // Verify descending order for bids
    let levels: Vec<Level> = bids.levels().copied().collect();
    for i in 1..levels.len() {
        assert!(
            levels[i - 1].price >= levels[i].price,
            "Bids not properly sorted: {} < {}",
            scale9_to_f64(levels[i - 1].price),
            scale9_to_f64(levels[i].price)
        );
    }

    // Test asks sorting (ascending)
    let mut asks = depthbook::types::Side::new();
    for price in prices {
        asks.insert(Level::new(f64_to_scale9(price), f64_to_scale9(1.0)), false);
    }

    // Verify ascending order for asks
    let levels: Vec<Level> = asks.levels().copied().collect();
    for i in 1..levels.len() {
        assert!(
            levels[i - 1].price <= levels[i].price,
            "Asks not properly sorted: {} > {}",
            scale9_to_f64(levels[i - 1].price),
            scale9_to_f64(levels[i].price)
        );
    }
}

#[test]
fn test_capacity_limits() {
    let mut bids = depthbook::types::Side::new();

    // Try to add more than 200 levels
    for i in 0..250 {
        let price = f64_to_scale9(50000.0 - i as f64);
        bids.insert(Level::new(price, f64_to_scale9(1.0)), true);
    }

    // Should be capped at 200
    assert_eq!(bids.count(), 200);

    // Best level should be the highest price
    let best = bids.best().unwrap();
    assert_eq!(scale9_to_f64(best.price), 50000.0);
}

#[test]
fn test_scale9_string_formatting() {
    let price = f64_to_scale9(123.456789);

    // Test different decimal places
    assert_eq!(scale9_to_string(price, 9), "123.456789000");
    assert_eq!(scale9_to_string(price, 6), "123.456789");
    assert_eq!(scale9_to_string(price, 2), "123.46");
    assert_eq!(scale9_to_string(price, 0), "123");
}
