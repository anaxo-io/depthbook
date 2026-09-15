//! Error type for order book operations.

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Something went wrong applying or reading an order book.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A delta skipped a sequence number, so the book can no longer be trusted.
    ///
    /// The update was not applied. Recover by requesting a fresh snapshot.
    #[error("sequence gap: expected {expected}, received {received}")]
    SequenceGap {
        /// The sequence number the book expected next.
        expected: u64,
        /// The sequence number that arrived.
        received: u64,
    },

    /// A snapshot is older than the book it would replace.
    ///
    /// The snapshot was not applied. If the venue reset its sequence numbers, call
    /// `BookStore::remove` before applying the first snapshot of the new sequence.
    #[error("snapshot seq {received} is older than book seq {current}; if the venue reset its sequence, call remove first")]
    OutOfOrder {
        /// The book's current sequence number.
        current: u64,
        /// The snapshot's sequence number.
        received: u64,
    },

    /// The input could not be turned into a valid book.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// No book exists for this venue and instrument.
    #[error("no order book for {venue}/{inst}")]
    NotFound {
        /// Venue identifier.
        venue: String,
        /// Instrument identifier.
        inst: String,
    },

    /// The book has not been updated recently enough to be used.
    #[error("order book is stale: {age_ms} ms old, maximum is {max_age_ms} ms")]
    Stale {
        /// Age of the book in milliseconds.
        age_ms: u64,
        /// Maximum age the caller was willing to accept.
        max_age_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_gap_message() {
        let err = Error::SequenceGap {
            expected: 100,
            received: 102,
        };
        assert_eq!(err.to_string(), "sequence gap: expected 100, received 102");
    }

    #[test]
    fn invalid_data_message() {
        let err = Error::InvalidData("empty bids and asks".to_string());
        assert_eq!(err.to_string(), "invalid data: empty bids and asks");
    }

    #[test]
    fn not_found_message() {
        let err = Error::NotFound {
            venue: "binance".to_string(),
            inst: "BTC-USDT".to_string(),
        };
        assert_eq!(err.to_string(), "no order book for binance/BTC-USDT");
    }

    #[test]
    fn stale_message() {
        let err = Error::Stale {
            age_ms: 5000,
            max_age_ms: 1000,
        };
        assert_eq!(
            err.to_string(),
            "order book is stale: 5000 ms old, maximum is 1000 ms"
        );
    }
}
