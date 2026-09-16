//! BookStore usage example demonstrating concurrent reads and updates.
//!
//! This example shows:
//! - Creating an in-memory order book store
//! - Applying snapshots and deltas
//! - concurrent reading from multiple threads
//! - Sequence gap detection
//! - Staleness checks

use depthbook::decimal::{f64_to_scale9, scale9_to_string};
use depthbook::store::BookStore;
use depthbook::types::Level;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== BookStore - Usage Example ===\n");

    // Create a new store
    let store = Arc::new(BookStore::new());
    println!("✓ Created BookStore\n");

    // Apply initial snapshot
    println!("=== Applying Initial Snapshot ===");
    let bids = vec![
        Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.5)),
        Level::new(f64_to_scale9(49999.0), f64_to_scale9(2.0)),
        Level::new(f64_to_scale9(49998.0), f64_to_scale9(1.0)),
    ];

    let asks = vec![
        Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0)),
        Level::new(f64_to_scale9(50002.0), f64_to_scale9(1.5)),
        Level::new(f64_to_scale9(50003.0), f64_to_scale9(2.0)),
    ];

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, timestamp)
        .expect("Failed to apply snapshot");

    println!("✓ Applied snapshot for binance/BTC-USDT");
    println!("  - Bids: {}", bids.len());
    println!("  - Asks: {}", asks.len());
    println!("  - Sequence: 1\n");

    // Read the snapshot
    println!("=== Reading Snapshot ===");
    if let Some(snapshot) = store.snapshot("binance", "BTC-USDT", 0) {
        println!("✓ Retrieved snapshot:");
        println!("  - Venue: {}", snapshot.venue);
        println!("  - Instrument: {}", snapshot.inst);
        println!("  - Sequence: {}", snapshot.seq);
        println!("  - Bids: {}", snapshot.bids.count());
        println!("  - Asks: {}", snapshot.asks.count());
    }
    println!();

    // Get BBO
    println!("=== Fast BBO Query ===");
    if let Some((best_bid, best_ask)) = store.bbo("binance", "BTC-USDT") {
        println!("✓ Best Bid/Ask:");
        println!(
            "  - Bid: ${} @ {} BTC",
            scale9_to_string(best_bid.price, 2),
            scale9_to_string(best_bid.qty, 4)
        );
        println!(
            "  - Ask: ${} @ {} BTC",
            scale9_to_string(best_ask.price, 2),
            scale9_to_string(best_ask.qty, 4)
        );
        println!(
            "  - Spread: ${}",
            scale9_to_string(best_ask.price - best_bid.price, 2)
        );
    }
    println!();

    // Apply delta updates
    println!("=== Applying Delta Updates ===");
    for seq in 2..=5 {
        let delta_bids = vec![Level::new(
            f64_to_scale9(50000.0 - seq as f64),
            f64_to_scale9(1.0),
        )];

        store
            .apply_delta("binance", "BTC-USDT", &delta_bids, &[], seq, now_ns())
            .expect("Failed to apply delta");

        println!(
            "✓ Applied delta {} - added bid at ${}",
            seq,
            50000.0 - seq as f64
        );
    }
    println!();

    // Check staleness
    println!("=== Staleness Check ===");
    let is_stale = store.is_stale("binance", "BTC-USDT", 1000);
    println!("✓ Book stale (max age 1000ms)? {}", is_stale);
    println!();

    // Demonstrate concurrent reads
    println!("=== Concurrent Reads ===");
    let mut handles = vec![];

    for i in 0..5 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            for _ in 0..10 {
                if let Some((_best_bid, _)) = store_clone.bbo("binance", "BTC-USDT") {
                    // Successfully read BBO
                }
                thread::sleep(Duration::from_millis(1));
            }
            println!("  ✓ Reader {} completed 10 non-blocking reads", i);
        });
        handles.push(handle);
    }

    // Wait for readers
    for handle in handles {
        handle.join().unwrap();
    }
    println!();

    // Demonstrate sequence gap detection
    println!("=== Sequence Gap Detection ===");
    let result = store.apply_delta("binance", "BTC-USDT", &[], &[], 100, now_ns());
    match result {
        Ok(_) => println!("✗ Unexpected success"),
        Err(e) => println!("✓ Detected gap: {}", e),
    }
    println!();

    // Multiple instruments
    println!("=== Multiple Instruments ===");
    let instruments = ["ETH-USDT", "SOL-USDT", "BNB-USDT"];

    for inst in &instruments {
        let bids = vec![Level::new(f64_to_scale9(1000.0), f64_to_scale9(10.0))];
        let asks = vec![Level::new(f64_to_scale9(1001.0), f64_to_scale9(10.0))];

        store
            .apply_snapshot("binance", inst, &bids, &asks, 1, timestamp)
            .expect("Failed to apply snapshot");

        println!("✓ Initialized binance/{}", inst);
    }
    println!();

    // Query all instruments
    println!("=== Query All Instruments ===");
    for inst in &instruments {
        if let Some((best_bid, best_ask)) = store.bbo("binance", inst) {
            println!(
                "✓ binance/{}: {} @ {}",
                inst,
                scale9_to_string(best_bid.price, 2),
                scale9_to_string(best_ask.price, 2)
            );
        }
    }
    println!();

    // Final statistics
    println!("=== Final Statistics ===");
    let snapshot = store.snapshot("binance", "BTC-USDT", 0).unwrap();
    println!("✓ BTC-USDT Order Book:");
    println!("  - Sequence: {}", snapshot.seq);
    println!("  - Bid levels: {}", snapshot.bids.count());
    println!("  - Ask levels: {}", snapshot.asks.count());
    println!(
        "  - Mid price: ${}",
        scale9_to_string(snapshot.mid_price().unwrap(), 2)
    );
    println!(
        "  - Spread: ${}",
        scale9_to_string(snapshot.spread().unwrap(), 2)
    );

    println!("\n=== Example Complete ===");
    println!("✓ All operations completed successfully");
    println!("✓ concurrent reads demonstrated");
    println!("✓ Sequence gap detection working");
    println!("✓ Multiple instruments supported");
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_nanos() as u64
}
