use depthbook::decimal::f64_to_scale9;
use depthbook::store::BookStore;
use depthbook::types::Level;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_concurrent_reads_single_writer() {
    let store = Arc::new(BookStore::new());

    // Initialize with a snapshot
    let bids = vec![
        Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0)),
        Level::new(f64_to_scale9(49999.0), f64_to_scale9(2.0)),
    ];

    let asks = vec![
        Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.5)),
        Level::new(f64_to_scale9(50002.0), f64_to_scale9(2.5)),
    ];

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, 1234567890000000000)
        .unwrap();

    // Spawn multiple reader threads
    let mut handles = vec![];
    for i in 0..10 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let snapshot = store_clone.snapshot("binance", "BTC-USDT", 0);
                assert!(snapshot.is_some());

                let snapshot = snapshot.unwrap();
                assert_eq!(snapshot.venue.as_str(), "binance");
                assert_eq!(snapshot.inst.as_str(), "BTC-USDT");
                assert!(snapshot.bids.count() >= 2);
                assert!(snapshot.asks.count() >= 2);

                // Also test BBO access
                let bbo = store_clone.bbo("binance", "BTC-USDT");
                assert!(bbo.is_some());
            }
            i
        });
        handles.push(handle);
    }

    // Wait for all reader threads
    for handle in handles {
        let thread_id = handle.join().unwrap();
        println!("Reader thread {} completed", thread_id);
    }
}

#[test]
fn test_concurrent_reads_with_updates() {
    let store = Arc::new(BookStore::new());

    // Initialize with a snapshot
    let bids = vec![Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0))];
    let asks = vec![Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0))];

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, 1234567890000000000)
        .unwrap();

    // Spawn reader threads
    let mut reader_handles = vec![];
    for i in 0..5 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let mut read_count = 0;
            for _ in 0..1000 {
                if let Some(snapshot) = store_clone.snapshot("binance", "BTC-USDT", 0) {
                    read_count += 1;
                    assert!(snapshot.bids.count() > 0);
                    assert!(snapshot.asks.count() > 0);
                }
                // Small sleep to allow writer to progress
                thread::sleep(Duration::from_micros(1));
            }
            (i, read_count)
        });
        reader_handles.push(handle);
    }

    // Spawn a single writer thread
    let store_writer = Arc::clone(&store);
    let writer_handle = thread::spawn(move || {
        for seq in 2..102 {
            let delta_bids = vec![Level::new(
                f64_to_scale9(50000.0 - seq as f64),
                f64_to_scale9(1.0),
            )];
            let delta_asks = vec![Level::new(
                f64_to_scale9(50001.0 + seq as f64),
                f64_to_scale9(1.0),
            )];

            let result = store_writer.apply_delta(
                "binance",
                "BTC-USDT",
                &delta_bids,
                &delta_asks,
                seq,
                now_ns(),
            );
            assert!(result.is_ok());

            thread::sleep(Duration::from_micros(10));
        }
        101 // Return number of updates
    });

    // Wait for all threads
    for handle in reader_handles {
        let (thread_id, read_count) = handle.join().unwrap();
        println!("Reader thread {} completed {} reads", thread_id, read_count);
        assert!(read_count > 0);
    }

    let update_count = writer_handle.join().unwrap();
    println!("Writer thread completed {} updates", update_count);
}

#[test]
fn test_multiple_instruments_concurrent_access() {
    let store = Arc::new(BookStore::new());

    let instruments = vec!["BTC-USDT", "ETH-USDT", "SOL-USDT", "BNB-USDT"];

    // Initialize all instruments
    for inst in &instruments {
        let bids = vec![Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0))];
        let asks = vec![Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0))];

        store
            .apply_snapshot("binance", inst, &bids, &asks, 1, 1234567890000000000)
            .unwrap();
    }

    // Spawn threads for each instrument
    let mut handles = vec![];
    for (idx, inst) in instruments.iter().enumerate() {
        let store_clone = Arc::clone(&store);
        let inst_clone = inst.to_string();

        let handle = thread::spawn(move || {
            // Each thread updates its own instrument
            for seq in 2..52 {
                let delta_bids = vec![Level::new(
                    f64_to_scale9(50000.0 - seq as f64),
                    f64_to_scale9(1.0),
                )];

                let result = store_clone.apply_delta(
                    "binance",
                    &inst_clone,
                    &delta_bids,
                    &[],
                    seq,
                    now_ns(),
                );
                assert!(result.is_ok());

                // Also read from it
                let snapshot = store_clone.snapshot("binance", &inst_clone, 0);
                assert!(snapshot.is_some());

                thread::sleep(Duration::from_micros(10));
            }
            (idx, inst_clone)
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        let (idx, inst) = handle.join().unwrap();
        println!("Thread {} for {} completed", idx, inst);

        // Verify final state
        let snapshot = store.snapshot("binance", &inst, 0).unwrap();
        assert_eq!(snapshot.seq, 51);
        assert!(snapshot.bids.count() > 1);
    }
}

#[test]
fn test_sequence_gap_with_concurrent_reads() {
    let store = Arc::new(BookStore::new());

    // Initialize with a snapshot
    let bids = vec![Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0))];
    let asks = vec![Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0))];

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, 1234567890000000000)
        .unwrap();

    // Spawn reader thread
    let store_reader = Arc::clone(&store);
    let reader_handle = thread::spawn(move || {
        for _ in 0..100 {
            let _ = store_reader.snapshot("binance", "BTC-USDT", 0);
            thread::sleep(Duration::from_micros(10));
        }
    });

    // Writer attempts update with gap
    thread::sleep(Duration::from_millis(10));
    let result = store.apply_delta("binance", "BTC-USDT", &[], &[], 10, now_ns());
    assert!(result.is_err()); // Should detect sequence gap

    // Reader should still be able to read
    reader_handle.join().unwrap();

    // The last good state (seq 1) is still served, and it is flagged as gapped so a
    // reader can tell it is behind the venue.
    let snapshot = store.snapshot("binance", "BTC-USDT", 0).unwrap();
    assert_eq!(snapshot.seq, 1);
    assert!(snapshot.gapped);
    assert!(store.is_gapped("binance", "BTC-USDT"));
    assert_eq!(store.stats().sequence_gaps, 1);
}

#[test]
fn test_concurrent_writers_apply_each_sequence_once() {
    let store = Arc::new(BookStore::new());
    store
        .apply_snapshot("binance", "BTC-USDT", &[], &[], 0, now_ns())
        .unwrap();

    // Every writer submits the same sequence range with a distinct price; each sequence
    // must be applied exactly once, so exactly one price survives and no gap is reported.
    let writers = 8;
    let deltas = 2_000;
    let handles: Vec<_> = (0..writers)
        .map(|w| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                let level = Level::new(f64_to_scale9(1.0 + w as f64), f64_to_scale9(1.0));
                let mut applied = 0;
                for seq in 1..=deltas {
                    let mut bids = [Level::new(level.price, f64_to_scale9(0.0)); 8];
                    for (i, bid) in bids.iter_mut().enumerate() {
                        bid.price = f64_to_scale9(1.0 + i as f64);
                    }
                    bids[w] = level;
                    if store
                        .apply_delta("binance", "BTC-USDT", &bids, &[], seq, now_ns())
                        .is_ok()
                    {
                        applied += 1;
                    }
                }
                applied
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let stats = store.stats();
    assert_eq!(stats.sequence_gaps, 0);
    assert_eq!(stats.deltas_applied, deltas);
    let book = store.snapshot("binance", "BTC-USDT", 0).unwrap();
    assert_eq!(book.seq, deltas);
    assert_eq!(
        book.bids.count(),
        1,
        "each delta wipes the other writers' prices"
    );
}

#[test]
fn test_bbo_access_during_updates() {
    let store = Arc::new(BookStore::new());

    // Initialize
    let bids = vec![Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0))];
    let asks = vec![Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0))];

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, 1234567890000000000)
        .unwrap();

    // Spawn multiple BBO reader threads
    let mut reader_handles = vec![];
    for i in 0..20 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let mut bbo_count = 0;
            for _ in 0..500 {
                if let Some((best_bid, best_ask)) = store_clone.bbo("binance", "BTC-USDT") {
                    bbo_count += 1;
                    // Verify bid < ask
                    assert!(best_bid.price < best_ask.price);
                }
            }
            (i, bbo_count)
        });
        reader_handles.push(handle);
    }

    // Writer thread - update bids to lower prices to maintain bid < ask invariant
    let store_writer = Arc::clone(&store);
    let writer_handle = thread::spawn(move || {
        for seq in 2..52 {
            let delta_bids = vec![Level::new(
                f64_to_scale9(50000.0 - seq as f64 * 0.1),
                f64_to_scale9(1.0),
            )];

            let _ =
                store_writer.apply_delta("binance", "BTC-USDT", &delta_bids, &[], seq, now_ns());
            thread::sleep(Duration::from_micros(5));
        }
    });

    // Wait for all threads
    for handle in reader_handles {
        let (thread_id, bbo_count) = handle.join().unwrap();
        println!(
            "BBO reader thread {} completed {} reads",
            thread_id, bbo_count
        );
        assert!(bbo_count > 0);
    }

    writer_handle.join().unwrap();
}

#[test]
fn test_staleness_check_during_updates() {
    let store = Arc::new(BookStore::new());

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    // Initialize
    let bids = vec![Level::new(f64_to_scale9(50000.0), f64_to_scale9(1.0))];
    let asks = vec![Level::new(f64_to_scale9(50001.0), f64_to_scale9(1.0))];

    store
        .apply_snapshot("binance", "BTC-USDT", &bids, &asks, 1, now)
        .unwrap();

    // Should not be stale initially
    assert!(!store.is_stale("binance", "BTC-USDT", 1000));

    // Spawn reader thread checking staleness
    let store_reader = Arc::clone(&store);
    let reader_handle = thread::spawn(move || {
        for _ in 0..100 {
            // With ongoing updates, should never be stale
            let _is_stale = store_reader.is_stale("binance", "BTC-USDT", 1000);
            thread::sleep(Duration::from_millis(1));
        }
    });

    // Writer thread keeps updating
    let store_writer = Arc::clone(&store);
    let writer_handle = thread::spawn(move || {
        for seq in 2..52 {
            let delta_bids = vec![Level::new(
                f64_to_scale9(50000.0 + seq as f64),
                f64_to_scale9(1.0),
            )];

            let _ =
                store_writer.apply_delta("binance", "BTC-USDT", &delta_bids, &[], seq, now_ns());
            thread::sleep(Duration::from_millis(5));
        }
    });

    reader_handle.join().unwrap();
    writer_handle.join().unwrap();

    // With recent updates, should not be stale
    assert!(!store.is_stale("binance", "BTC-USDT", 1000));
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_nanos() as u64
}
