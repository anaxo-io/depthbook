//! In-process L2 order book state.
//!
//! This crate keeps limit order books in memory and applies exchange feeds to them:
//! full snapshots, incremental deltas, and the sequence checks needed to notice when
//! a feed has dropped a message.
//!
//! # Design
//!
//! - **Fixed-capacity sides.** Each side of a book is a sorted [`Level`] array, so
//!   applying an update is a binary search plus a memmove with no allocation.
//! - **Integer prices.** Prices and quantities are [`Scale9`] fixed-point values rather
//!   than `f64` or bare `i64`, so arithmetic is exact and an unscaled number cannot be
//!   passed where a scaled one belongs.
//! - **Sequence tracking.** [`BookStore::apply_delta`] rejects an update that skips a
//!   sequence number instead of silently corrupting the book.
//!
//! # Example
//!
//! ```
//! use orderbook::{f64_to_scale9, BookStore, Level};
//!
//! let store = BookStore::new();
//!
//! store.apply_snapshot(
//!     "binance",
//!     "BTC-USDT",
//!     &[Level::new(f64_to_scale9(50_000.0), f64_to_scale9(1.0))],
//!     &[Level::new(f64_to_scale9(50_001.0), f64_to_scale9(2.0))],
//!     1,
//!     1_700_000_000_000_000_000,
//! )?;
//!
//! let (bid, ask) = store.bbo("binance", "BTC-USDT").unwrap();
//! assert_eq!(ask.price - bid.price, f64_to_scale9(1.0));
//!
//! // A delta that skips sequence 2 is refused rather than applied.
//! let gap = store.apply_delta("binance", "BTC-USDT", &[], &[], 7, 1_700_000_000_000_000_001);
//! assert!(gap.is_err());
//! assert_eq!(store.stats().sequence_gaps, 1);
//! # Ok::<(), orderbook::Error>(())
//! ```
//!
//! # Scope
//!
//! This is a data structure, not a feed handler. It does no I/O and speaks no exchange
//! protocol; feeding it is the caller's job. State lives in the process that owns the
//! [`BookStore`] and is not shared between processes or persisted.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod book_state;
pub mod decimal;
pub mod error;
pub mod intern;
pub mod store;
pub mod types;

pub use decimal::{
    f64_to_scale9, scale9_to_f64, scale9_to_string, str_to_scale9, Scale9, SCALE, SCALE9,
};
pub use error::{Error, Result};
pub use intern::InternedString;
pub use store::{BookStore, Stats};
pub use types::{Book, Level, Side, MAX_LEVELS};
