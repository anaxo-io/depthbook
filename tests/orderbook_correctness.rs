//! Comprehensive correctness tests for BookStore
//!
//! These tests validate the correctness of order book operations including:
//! - Snapshot and delta application
//! - Level removal and updates
//! - Sorted invariants
//! - Sequence gap detection
//! - Staleness detection
//! - Multi-instrument isolation

use depthbook::decimal::{f64_to_scale9, scale9_to_f64};
use depthbook::error::Error;
use depthbook::store::BookStore;
use depthbook::types::Level;

// Helper to get current time in nanoseconds
fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

// Helper to create levels from f64 prices and quantities
fn create_levels(data: &[(f64, f64)]) -> Vec<Level> {
    data.iter()
        .map(|(price, qty)| Level::new(f64_to_scale9(*price), f64_to_scale9(*qty)))
        .collect()
}

#[test]
fn test_snapshot_then_deltas() {
    let store = BookStore::new();

    // Apply initial snapshot
    let snapshot_bids = create_levels(&[(50000.0, 1.0), (49999.0, 2.0)]);
    let snapshot_asks = create_levels(&[(50001.0, 1.5), (50002.0, 0.5)]);

    store
        .apply_snapshot(
            "binance",
            "BTC-USDT",
            &snapshot_bids,
            &snapshot_asks,
            1,
            now_nanos(),
        )
        .unwrap();

    // Verify snapshot
    let result = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(result.bids.count(), 2);
    assert_eq!(result.asks.count(), 2);
    assert_eq!(result.seq, 1);

    let (best_bid, best_ask) = store.bbo("binance", "BTC-USDT").unwrap();
    assert_eq!(scale9_to_f64(best_bid.price), 50000.0);
    assert_eq!(scale9_to_f64(best_ask.price), 50001.0);

    // Apply delta that modifies first bid level
    let delta_bids = create_levels(&[(50000.0, 1.5)]);
    store
        .apply_delta("binance", "BTC-USDT", &delta_bids, &[], 2, now_nanos())
        .unwrap();

    // Verify delta applied
    let result = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(result.seq, 2);
    let best_bid = result.bids.best().unwrap();
    assert_eq!(scale9_to_f64(best_bid.qty), 1.5);
    assert_eq!(scale9_to_f64(best_bid.price), 50000.0);
}

#[test]
fn test_delta_removes_level() {
    let store = BookStore::new();

    // Setup with multiple bid levels
    let snapshot_bids = create_levels(&[
        (50000.0, 1.0),
        (49999.0, 2.0),
        (49998.0, 1.5),
        (49997.0, 0.5),
    ]);
    let snapshot_asks = create_levels(&[(50001.0, 1.0)]);

    store
        .apply_snapshot(
            "binance",
            "BTC-USDT",
            &snapshot_bids,
            &snapshot_asks,
            1,
            now_nanos(),
        )
        .unwrap();

    // Verify initial state
    let snapshot = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(snapshot.bids.count(), 4);

    // Delta with qty=0 should remove level
    let delta_bids = create_levels(&[(49999.0, 0.0)]);
    store
        .apply_delta("binance", "BTC-USDT", &delta_bids, &[], 2, now_nanos())
        .unwrap();

    // Verify level removed
    let result = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(result.bids.count(), 3);

    // Verify the removed price is no longer present
    let has_removed_price = result
        .bids
        .levels()
        .any(|l| scale9_to_f64(l.price) == 49999.0);
    assert!(!has_removed_price, "Level at 49999.0 should be removed");

    // Verify remaining levels
    assert_eq!(
        scale9_to_f64(result.bids.levels().next().unwrap().price),
        50000.0
    );
    assert_eq!(
        scale9_to_f64(result.bids.levels().nth(1).unwrap().price),
        49998.0
    );
    assert_eq!(
        scale9_to_f64(result.bids.levels().nth(2).unwrap().price),
        49997.0
    );
}

#[test]
fn test_sorted_invariants() {
    let store = BookStore::new();

    // Apply initial snapshot with unsorted data (store should sort it)
    let snapshot_bids = create_levels(&[
        (49995.0, 1.0),
        (50000.0, 1.0),
        (49998.0, 1.0),
        (49999.0, 1.0),
    ]);
    let snapshot_asks = create_levels(&[
        (50005.0, 1.0),
        (50001.0, 1.0),
        (50003.0, 1.0),
        (50002.0, 1.0),
    ]);

    store
        .apply_snapshot(
            "binance",
            "BTC-USDT",
            &snapshot_bids,
            &snapshot_asks,
            1,
            now_nanos(),
        )
        .unwrap();

    // Apply random deltas
    let random_deltas = [
        (49992.0, 1.5),
        (50010.0, 2.0),
        (49996.5, 1.0),
        (50004.5, 0.5),
        (49993.0, 2.5),
    ];

    for (idx, (price, qty)) in random_deltas.iter().enumerate() {
        let is_bid = *price < 50000.0;
        let delta = create_levels(&[(*price, *qty)]);

        if is_bid {
            store
                .apply_delta(
                    "binance",
                    "BTC-USDT",
                    &delta,
                    &[],
                    idx as u64 + 2,
                    now_nanos(),
                )
                .unwrap();
        } else {
            store
                .apply_delta(
                    "binance",
                    "BTC-USDT",
                    &[],
                    &delta,
                    idx as u64 + 2,
                    now_nanos(),
                )
                .unwrap();
        }
    }

    // Verify bids are sorted descending
    let snapshot = store.snapshot("binance", "BTC-USDT", 200).unwrap();

    for i in 1..snapshot.bids.count() {
        let prev_price = scale9_to_f64(snapshot.bids.levels().nth(i - 1).unwrap().price);
        let curr_price = scale9_to_f64(snapshot.bids.levels().nth(i).unwrap().price);
        assert!(
            prev_price > curr_price,
            "Bids not descending: {} <= {}",
            prev_price,
            curr_price
        );
    }

    // Verify asks are sorted ascending
    for i in 1..snapshot.asks.count() {
        let prev_price = scale9_to_f64(snapshot.asks.levels().nth(i - 1).unwrap().price);
        let curr_price = scale9_to_f64(snapshot.asks.levels().nth(i).unwrap().price);
        assert!(
            prev_price < curr_price,
            "Asks not ascending: {} >= {}",
            prev_price,
            curr_price
        );
    }
}

#[test]
fn test_sequence_gap_detection() {
    let store = BookStore::new();

    // Apply initial snapshot with seq=1
    store
        .apply_snapshot("binance", "BTC-USDT", &[], &[], 1, now_nanos())
        .unwrap();

    // Apply seq=2 - should succeed
    assert!(store
        .apply_delta("binance", "BTC-USDT", &[], &[], 2, now_nanos())
        .is_ok());

    // Apply seq=3 - should succeed
    assert!(store
        .apply_delta("binance", "BTC-USDT", &[], &[], 3, now_nanos())
        .is_ok());

    // Skip to seq=10 - should fail with sequence gap
    let result = store.apply_delta("binance", "BTC-USDT", &[], &[], 10, now_nanos());
    assert!(result.is_err());
    assert!(matches!(result, Err(Error::SequenceGap { .. })));

    if let Err(Error::SequenceGap { expected, received }) = result {
        assert_eq!(expected, 4);
        assert_eq!(received, 10);
    }
}

#[test]
fn test_duplicate_sequence_ignored() {
    let store = BookStore::new();

    let bids = create_levels(&[(50000.0, 1.0)]);

    // Apply snapshot
    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &[], 1, now_nanos())
        .unwrap();

    // Apply delta with seq=2
    let delta1 = create_levels(&[(49999.0, 1.5)]);
    store
        .apply_delta("binance", "BTC-USDT", &delta1, &[], 2, now_nanos())
        .unwrap();

    // Verify delta applied
    let snapshot = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(snapshot.bids.count(), 2);

    // Try to apply seq=2 again with different data - should be ignored
    let delta2 = create_levels(&[(49998.0, 2.0)]);
    let result = store.apply_delta("binance", "BTC-USDT", &delta2, &[], 2, now_nanos());
    assert!(result.is_ok()); // Not an error, just ignored

    // Verify the second delta was not applied
    let snapshot = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(snapshot.bids.count(), 2); // Still only 2 levels
    assert_eq!(snapshot.seq, 2);

    // Should not have 49998.0 price level
    let has_49998 = snapshot
        .bids
        .levels()
        .any(|l| scale9_to_f64(l.price) == 49998.0);
    assert!(!has_49998);
}

#[test]
fn test_staleness_detection() {
    let store = BookStore::new();

    // Non-existent book is stale
    assert!(store.is_stale("binance", "BTC-USDT", 5000));

    // Create book with old timestamp (10 seconds ago)
    let old_ts = now_nanos() - 10_000_000_000;
    store
        .apply_snapshot("binance", "BTC-USDT", &[], &[], 1, old_ts)
        .unwrap();

    // Should be stale with 5 second threshold
    assert!(store.is_stale("binance", "BTC-USDT", 5000));

    // Should not be stale with 15 second threshold
    assert!(!store.is_stale("binance", "BTC-USDT", 15000));

    // Update with current timestamp
    store
        .apply_snapshot("binance", "BTC-USDT", &[], &[], 2, now_nanos())
        .unwrap();

    // Should not be stale anymore
    assert!(!store.is_stale("binance", "BTC-USDT", 5000));
}

#[test]
fn test_multiple_instruments() {
    let store = BookStore::new();

    let instruments = ["BTC-USDT", "ETH-USDT", "SOL-USDT", "BNB-USDT"];

    // Setup different order books for each instrument
    for (idx, inst) in instruments.iter().enumerate() {
        let base_price = 10000.0 * (idx as f64 + 1.0);
        let bids = create_levels(&[
            (base_price, 1.0),
            (base_price - 1.0, 2.0),
            (base_price - 2.0, 3.0),
        ]);
        let asks = create_levels(&[
            (base_price + 1.0, 1.5),
            (base_price + 2.0, 2.5),
            (base_price + 3.0, 3.5),
        ]);

        store
            .apply_snapshot("binance", inst, &bids, &asks, 1, now_nanos())
            .unwrap();
    }

    // Verify isolation - each instrument has its own data
    for (idx, inst) in instruments.iter().enumerate() {
        let snapshot = store
            .snapshot("binance", inst, 50)
            .unwrap_or_else(|| panic!("Should find {}", inst));

        assert_eq!(snapshot.inst.as_str(), *inst);
        assert_eq!(snapshot.bids.count(), 3);
        assert_eq!(snapshot.asks.count(), 3);

        let expected_base_price = 10000.0 * (idx as f64 + 1.0);
        let (best_bid, best_ask) = store.bbo("binance", inst).unwrap();

        assert_eq!(scale9_to_f64(best_bid.price), expected_base_price);
        assert_eq!(scale9_to_f64(best_ask.price), expected_base_price + 1.0);
    }

    // Verify updating one doesn't affect others
    let delta = create_levels(&[(10000.0, 10.0)]);
    store
        .apply_delta("binance", "BTC-USDT", &delta, &[], 2, now_nanos())
        .unwrap();

    let btc_snapshot = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    let eth_snapshot = store.snapshot("binance", "ETH-USDT", 10).unwrap();

    assert_eq!(btc_snapshot.seq, 2);
    assert_eq!(eth_snapshot.seq, 1); // ETH unaffected
}

#[test]
fn test_multiple_venues_same_instrument() {
    let store = BookStore::new();

    let venues = ["binance", "coinbase", "okx"];

    // Setup same instrument on different venues with different prices
    for (idx, venue) in venues.iter().enumerate() {
        let base_price = 50000.0 + (idx as f64 * 10.0);
        let bids = create_levels(&[(base_price, 1.0)]);
        let asks = create_levels(&[(base_price + 1.0, 1.0)]);

        store
            .apply_snapshot(venue, "BTC-USDT", &bids, &asks, 1, now_nanos())
            .unwrap();
    }

    // Verify each venue has independent data
    for (idx, venue) in venues.iter().enumerate() {
        let (best_bid, best_ask) = store.bbo(venue, "BTC-USDT").unwrap();

        let expected_bid = 50000.0 + (idx as f64 * 10.0);
        let expected_ask = expected_bid + 1.0;

        assert_eq!(scale9_to_f64(best_bid.price), expected_bid);
        assert_eq!(scale9_to_f64(best_ask.price), expected_ask);
    }
}

#[test]
fn test_large_price_range() {
    let store = BookStore::new();

    // Test with very small and very large prices
    let bids = create_levels(&[
        (100000.12345678, 1.0),
        (0.00000001, 1000000.0),
        (50000.0, 1.0),
    ]);
    let asks = create_levels(&[(100001.0, 1.0), (0.00000002, 2000000.0), (50001.0, 1.0)]);

    store
        .apply_snapshot("binance", "TEST-USDT", &bids, &asks, 1, now_nanos())
        .unwrap();

    let snapshot = store.snapshot("binance", "TEST-USDT", 10).unwrap();
    assert_eq!(snapshot.bids.count(), 3);
    assert_eq!(snapshot.asks.count(), 3);

    // Best bid should be highest price
    let best_bid = snapshot.bids.best().unwrap();
    assert!(scale9_to_f64(best_bid.price) > 100000.0);
}

#[test]
fn test_not_found_error() {
    let store = BookStore::new();

    // Getting non-existent book should return None
    assert!(store.snapshot("binance", "NONEXISTENT", 10).is_none());
    assert!(store.bbo("binance", "NONEXISTENT").is_none());

    // Applying delta to non-existent book should fail
    let result = store.apply_delta("binance", "NONEXISTENT", &[], &[], 1, now_nanos());
    assert!(result.is_err());
    assert!(matches!(result, Err(Error::NotFound { .. })));
}

#[test]
fn test_snapshot_overwrites_existing() {
    let store = BookStore::new();

    // Apply initial snapshot
    let bids1 = create_levels(&[(50000.0, 1.0), (49999.0, 2.0)]);
    let asks1 = create_levels(&[(50001.0, 1.0)]);

    store
        .apply_snapshot("binance", "BTC-USDT", &bids1, &asks1, 1, now_nanos())
        .unwrap();

    let snapshot1 = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(snapshot1.bids.count(), 2);
    assert_eq!(snapshot1.asks.count(), 1);

    // Apply new snapshot with different data
    let bids2 = create_levels(&[(51000.0, 5.0)]);
    let asks2 = create_levels(&[(51001.0, 3.0), (51002.0, 4.0)]);

    store
        .apply_snapshot("binance", "BTC-USDT", &bids2, &asks2, 10, now_nanos())
        .unwrap();

    // Verify old data is completely replaced
    let snapshot2 = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(snapshot2.bids.count(), 1);
    assert_eq!(snapshot2.asks.count(), 2);
    assert_eq!(snapshot2.seq, 10);

    let (best_bid, best_ask) = store.bbo("binance", "BTC-USDT").unwrap();
    assert_eq!(scale9_to_f64(best_bid.price), 51000.0);
    assert_eq!(scale9_to_f64(best_ask.price), 51001.0);
}

#[test]
fn test_depth_limiting() {
    let store = BookStore::new();

    // Create snapshot with many levels
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    for i in 0..100 {
        bids.push((50000.0 - i as f64, 1.0));
        asks.push((50001.0 + i as f64, 1.0));
    }

    let bid_levels = create_levels(&bids);
    let ask_levels = create_levels(&asks);

    store
        .apply_snapshot(
            "binance",
            "BTC-USDT",
            &bid_levels,
            &ask_levels,
            1,
            now_nanos(),
        )
        .unwrap();

    // A depth of 0 means "everything".
    let snapshot = store.snapshot("binance", "BTC-USDT", 0).unwrap();
    assert_eq!(snapshot.bids.count(), 100);
    assert_eq!(snapshot.asks.count(), 100);

    // A depth limit keeps that many levels per side, best first.
    let snapshot_10 = store.snapshot("binance", "BTC-USDT", 10).unwrap();
    assert_eq!(snapshot_10.bids.count(), 10);
    assert_eq!(snapshot_10.asks.count(), 10);
    assert_eq!(
        scale9_to_f64(snapshot_10.bids.best().unwrap().price),
        50000.0
    );
    assert_eq!(
        scale9_to_f64(snapshot_10.asks.best().unwrap().price),
        50001.0
    );
    assert_eq!(
        scale9_to_f64(snapshot_10.bids.levels().nth(9).unwrap().price),
        50000.0 - 9.0
    );

    // Asking for more levels than exist is not an error.
    let snapshot_500 = store.snapshot("binance", "BTC-USDT", 500).unwrap();
    assert_eq!(snapshot_500.bids.count(), 100);

    // Truncation does not disturb the stored book.
    let again = store.snapshot("binance", "BTC-USDT", 0).unwrap();
    assert_eq!(again.bids.count(), 100);
}
