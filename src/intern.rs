//! String interning for efficient venue and instrument storage.
//!
//! This module provides an interned string type that uses `Arc<str>` for zero-copy sharing
//! and a global cache to deduplicate strings. This is particularly useful for venue and
//! instrument identifiers which are used frequently across the system.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Global string interning cache.
///
/// This cache stores all interned strings to ensure that identical strings
/// share the same underlying allocation.
static INTERN_CACHE: once_cell::sync::Lazy<DashMap<Arc<str>, Arc<str>>> =
    once_cell::sync::Lazy::new(DashMap::new);

/// An interned string that uses reference counting for efficient memory usage.
///
/// Multiple instances of the same string content will share the same underlying
/// memory allocation through the global interning cache.
///
/// # Examples
/// ```
/// use depthbook::intern::InternedString;
///
/// let s1 = InternedString::new("binance");
/// let s2 = InternedString::new("binance");
///
/// // Both strings share the same underlying allocation
/// assert_eq!(s1.as_str(), s2.as_str());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InternedString {
    inner: Arc<str>,
}

impl InternedString {
    /// Create a new interned string.
    ///
    /// If a string with the same content already exists in the cache,
    /// the existing allocation will be reused.
    ///
    /// # Arguments
    /// * `s` - The string to intern
    ///
    /// # Examples
    /// ```
    /// use depthbook::intern::InternedString;
    ///
    /// let venue = InternedString::new("binance");
    /// assert_eq!(venue.as_str(), "binance");
    /// ```
    pub fn new(s: &str) -> Self {
        // Use entry API for atomic get-or-insert operation
        // This prevents race conditions where two threads might both create new Arc instances
        let arc: Arc<str> = Arc::from(s);
        let entry = INTERN_CACHE
            .entry(Arc::clone(&arc))
            .or_insert_with(|| Arc::clone(&arc));

        Self {
            inner: Arc::clone(entry.value()),
        }
    }

    /// Get the string content as a string slice.
    ///
    /// # Examples
    /// ```
    /// use depthbook::intern::InternedString;
    ///
    /// let s = InternedString::new("BTC-USDT");
    /// assert_eq!(s.as_str(), "BTC-USDT");
    /// ```
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// Get the length of the string in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the string is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the number of strings currently in the interning cache.
    ///
    /// This is useful for monitoring memory usage and debugging.
    pub fn cache_size() -> usize {
        INTERN_CACHE.len()
    }

    /// Clear the interning cache.
    ///
    /// This should only be used in tests or when you're certain no interned
    /// strings are still in use.
    #[cfg(test)]
    pub fn clear_cache() {
        INTERN_CACHE.clear();
    }
}

impl Deref for InternedString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl fmt::Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}

impl AsRef<str> for InternedString {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl From<&str> for InternedString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for InternedString {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl From<&String> for InternedString {
    fn from(s: &String) -> Self {
        Self::new(s)
    }
}

impl PartialEq<str> for InternedString {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for InternedString {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for InternedString {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

// Implement Serialize by serializing the string content
impl Serialize for InternedString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.inner)
    }
}

// Implement Deserialize by creating a new interned string
impl<'de> Deserialize<'de> for InternedString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::new(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_interned_string_creation() {
        InternedString::clear_cache();

        let s1 = InternedString::new("test");
        assert_eq!(s1.as_str(), "test");
        assert_eq!(s1.len(), 4);
        assert!(!s1.is_empty());
    }

    #[test]
    #[serial]
    fn test_interned_string_deduplication() {
        InternedString::clear_cache();

        let s1 = InternedString::new("binance_test_dedup_unique");
        let s2 = InternedString::new("binance_test_dedup_unique");

        // Both should point to the same underlying allocation
        assert_eq!(s1, s2);
        assert_eq!(s1.as_str(), s2.as_str());

        // Verify they share the same Arc by comparing the data pointers
        // Arc::clone creates new Arc handles pointing to the same data
        assert_eq!(s1.as_str().as_ptr(), s2.as_str().as_ptr());
    }

    #[test]
    #[serial]
    fn test_interned_string_different_values() {
        InternedString::clear_cache();

        let s1 = InternedString::new("binance_test_diff_unique");
        let s2 = InternedString::new("coinbase_test_diff_unique");

        // Different strings should not be equal
        assert_ne!(s1, s2);
        assert_ne!(s1.as_str(), s2.as_str());

        // Verify they don't share the same allocation
        assert_ne!(s1.as_str().as_ptr(), s2.as_str().as_ptr());
    }

    #[test]
    fn test_interned_string_empty() {
        let s = InternedString::new("");
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn test_interned_string_display() {
        let s = InternedString::new("BTC-USDT");
        assert_eq!(format!("{}", s), "BTC-USDT");
    }

    #[test]
    fn test_interned_string_partial_eq_str() {
        let s = InternedString::new("binance");
        assert_eq!(s, "binance");
        assert_ne!(s, "coinbase");
    }

    #[test]
    fn test_interned_string_from_string() {
        let string = String::from("test");
        let s = InternedString::from(&string);
        assert_eq!(s.as_str(), "test");
    }

    #[test]
    fn test_interned_string_serialization() {
        let s = InternedString::new("binance");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"binance\"");
    }

    #[test]
    fn test_interned_string_deserialization() {
        let json = "\"coinbase\"";
        let s: InternedString = serde_json::from_str(json).unwrap();
        assert_eq!(s.as_str(), "coinbase");
    }

    #[test]
    fn test_interned_string_clone() {
        let s1 = InternedString::new("test");
        let s2 = s1.clone();
        assert_eq!(s1, s2);
        assert_eq!(s1.as_str(), s2.as_str());
    }
}
