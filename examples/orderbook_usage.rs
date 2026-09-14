use orderbook::decimal::{f64_to_scale9, scale9_to_f64, scale9_to_string};
use orderbook::intern::InternedString;
/// Basic usage example for the orderbook crate.
///
/// This example demonstrates how to:
/// - Create an order book snapshot
/// - Add levels to both sides
/// - Query the order book
/// - Serialize to JSON
use orderbook::types::{Book, Level};

fn main() {
    println!("=== Order Book Store - Basic Usage Example ===\n");

    // Create a new order book snapshot
    let venue = InternedString::new("binance");
    let instrument = InternedString::new("BTC-USDT");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let mut book = Book::new(venue, instrument, timestamp, 1);

    println!("Created order book for {} on {}", book.inst, book.venue);
    println!("Timestamp: {} ns\n", book.ts);

    // Add some realistic bid levels (buy orders)
    println!("Adding bid levels...");
    let bids = [
        (50000.12, 1.5),
        (49999.50, 2.3),
        (49998.00, 0.75),
        (49997.25, 1.8),
        (49996.50, 3.2),
    ];

    for (price, qty) in bids {
        let level = Level::new(f64_to_scale9(price), f64_to_scale9(qty));
        book.bids.insert(level, true);
        println!("  Added bid: ${:.2} @ {:.4} BTC", price, qty);
    }

    // Add some realistic ask levels (sell orders)
    println!("\nAdding ask levels...");
    let asks = [
        (50001.25, 1.0),
        (50002.50, 1.8),
        (50003.75, 2.5),
        (50005.00, 0.9),
        (50006.25, 1.6),
    ];

    for (price, qty) in asks {
        let level = Level::new(f64_to_scale9(price), f64_to_scale9(qty));
        book.asks.insert(level, false);
        println!("  Added ask: ${:.2} @ {:.4} BTC", price, qty);
    }

    // Query the order book
    println!("\n=== Order Book Summary ===");

    let best_bid = book.bids.best().unwrap();
    let best_ask = book.asks.best().unwrap();

    println!(
        "Best Bid: ${} @ {} BTC",
        scale9_to_string(best_bid.price, 2),
        scale9_to_string(best_bid.qty, 4)
    );
    println!(
        "Best Ask: ${} @ {} BTC",
        scale9_to_string(best_ask.price, 2),
        scale9_to_string(best_ask.qty, 4)
    );

    let mid_price = book.mid_price().unwrap();
    println!("Mid Price: ${}", scale9_to_string(mid_price, 2));

    let spread = book.spread().unwrap();
    println!("Spread: ${}", scale9_to_string(spread, 2));

    println!("\nBid Levels: {}", book.bids.count());
    println!("Ask Levels: {}", book.asks.count());

    // Calculate liquidity depth
    println!("\n=== Liquidity Depth ===");
    for bps in [10, 50, 100, 200] {
        let bid_depth = book.bids.depth_within_bps(bps, true);
        let ask_depth = book.asks.depth_within_bps(bps, false);
        println!("Within {} bps:", bps);
        println!("  Bid depth: {} BTC", scale9_to_string(bid_depth, 4));
        println!("  Ask depth: {} BTC", scale9_to_string(ask_depth, 4));
    }

    // Update a level
    println!("\n=== Updating a Level ===");
    println!("Updating bid at $49999.50 to 5.0 BTC");
    book.bids
        .update(f64_to_scale9(49999.50), f64_to_scale9(5.0), true);

    let updated_level = book
        .bids
        .levels()
        .iter()
        .find(|l| scale9_to_f64(l.price) == 49999.50)
        .unwrap();
    println!(
        "Updated: ${} @ {} BTC",
        scale9_to_string(updated_level.price, 2),
        scale9_to_string(updated_level.qty, 4)
    );

    // Remove a level
    println!("\n=== Removing a Level ===");
    println!("Removing bid at $49996.50");
    book.bids.remove(f64_to_scale9(49996.50), true);
    println!("Bid levels after removal: {}", book.bids.count());

    // Serialize to JSON
    println!("\n=== JSON Serialization ===");
    let json = book.to_json_pretty().unwrap();
    println!("{}", json);

    // Demonstrate scale-9 precision
    println!("\n=== Scale-9 Precision ===");
    let precise_price = 123.456789012;
    let scale9 = f64_to_scale9(precise_price);
    let back = scale9_to_f64(scale9);
    println!("Original:    {:.9}", precise_price);
    println!("Scale-9:     {}", scale9);
    println!("Converted:   {:.9}", back);
    println!("Difference:  {:.12}", (back - precise_price).abs());

    // Demonstrate string interning efficiency
    println!("\n=== String Interning ===");
    let venue1 = InternedString::new("binance");
    let venue2 = InternedString::new("binance");
    let _venue3 = InternedString::new("coinbase");

    println!("Created 3 InternedString instances");
    println!("Cache size: {}", InternedString::cache_size());
    println!("'binance' strings share memory: {}", venue1 == venue2);

    println!("\n=== Example Complete ===");
}
