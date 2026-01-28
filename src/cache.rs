//! Client-side statement caching for D1 backends
//!
//! This module provides best-effort client-side statement caching to reduce
//! repeated preparation/serialization overhead for frequently executed SQL.
//!
//! # Important Notes
//!
//! - Caching is **best-effort** and may reset on isolate eviction (WASM) or process restart
//! - The cache is safe under concurrency (no UB, no shared mutable hazards)
//! - For HTTP backend, the cache focuses on SQL string reuse and parameter ordering metadata
//! - For WASM backend, prepared statements are cached by SQL string key in an in-memory LRU
//!
//! # Example
//!
//! ```
//! use diesel_d1::cache::StatementCacheConfig;
//!
//! let config = StatementCacheConfig::builder()
//!     .max_entries(100)
//!     .enabled(true)
//!     .build();
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Default maximum number of cached statement entries
pub const DEFAULT_MAX_ENTRIES: usize = 128;

/// Default maximum total bytes for cache (16 KB)
pub const DEFAULT_MAX_BYTES: usize = 16 * 1024;

/// Configuration for the statement cache
///
/// # Example
///
/// ```
/// use diesel_d1::cache::StatementCacheConfig;
///
/// let config = StatementCacheConfig::builder()
///     .max_entries(200)
///     .max_bytes(32 * 1024)
///     .enabled(true)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct StatementCacheConfig {
    /// Maximum number of cache entries
    max_entries: usize,
    /// Maximum total bytes for cache (optional)
    max_bytes: Option<usize>,
    /// Whether caching is enabled
    enabled: bool,
}

impl Default for StatementCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_bytes: Some(DEFAULT_MAX_BYTES),
            enabled: true,
        }
    }
}

impl StatementCacheConfig {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a disabled cache configuration
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Create a builder for configuring the cache
    pub fn builder() -> StatementCacheConfigBuilder {
        StatementCacheConfigBuilder::default()
    }

    /// Get the maximum number of entries
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Get the maximum bytes (if set)
    pub fn max_bytes(&self) -> Option<usize> {
        self.max_bytes
    }

    /// Check if caching is enabled
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Builder for StatementCacheConfig
#[derive(Debug, Default)]
pub struct StatementCacheConfigBuilder {
    max_entries: Option<usize>,
    max_bytes: Option<Option<usize>>,
    enabled: Option<bool>,
}

impl StatementCacheConfigBuilder {
    /// Set the maximum number of cache entries
    pub fn max_entries(mut self, max: usize) -> Self {
        self.max_entries = Some(max);
        self
    }

    /// Set the maximum total bytes for the cache
    pub fn max_bytes(mut self, max: usize) -> Self {
        self.max_bytes = Some(Some(max));
        self
    }

    /// Disable the byte limit
    pub fn no_byte_limit(mut self) -> Self {
        self.max_bytes = Some(None);
        self
    }

    /// Enable or disable caching
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Build the configuration
    pub fn build(self) -> StatementCacheConfig {
        let default = StatementCacheConfig::default();
        StatementCacheConfig {
            max_entries: self.max_entries.unwrap_or(default.max_entries),
            max_bytes: self.max_bytes.unwrap_or(default.max_bytes),
            enabled: self.enabled.unwrap_or(default.enabled),
        }
    }
}

/// Entry in the statement cache
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The SQL string
    pub sql: String,
    /// Parameter count for this statement
    pub param_count: usize,
    /// Access count for LRU tracking
    access_count: u64,
    /// Last access timestamp for LRU
    last_access: u64,
}

impl CacheEntry {
    /// Create a new cache entry
    fn new(sql: String, param_count: usize, access_count: u64) -> Self {
        Self {
            sql,
            param_count,
            access_count,
            last_access: access_count,
        }
    }

    /// Get the approximate size in bytes
    pub fn size_bytes(&self) -> usize {
        self.sql.len() + std::mem::size_of::<Self>()
    }
}

/// LRU-based statement cache
///
/// This cache stores prepared statement metadata by SQL string key.
/// It is safe for concurrent access through interior mutability with RwLock.
///
/// # Example
///
/// ```
/// use diesel_d1::cache::{StatementCache, StatementCacheConfig};
///
/// let config = StatementCacheConfig::builder()
///     .max_entries(10)
///     .build();
/// let cache = StatementCache::new(config);
///
/// // Insert a statement
/// cache.insert("SELECT * FROM users WHERE id = ?", 1);
///
/// // Look up the statement
/// let entry = cache.get("SELECT * FROM users WHERE id = ?");
/// assert!(entry.is_some());
///
/// // Check cache statistics
/// let stats = cache.stats();
/// assert_eq!(stats.hits, 1);
/// assert_eq!(stats.misses, 0);
/// ```
pub struct StatementCache {
    config: StatementCacheConfig,
    entries: RwLock<HashMap<String, CacheEntry>>,
    access_counter: AtomicU64,
    stats: CacheStats,
}

impl StatementCache {
    /// Create a new statement cache with the given configuration
    pub fn new(config: StatementCacheConfig) -> Self {
        Self {
            config,
            entries: RwLock::new(HashMap::new()),
            access_counter: AtomicU64::new(0),
            stats: CacheStats::default(),
        }
    }

    /// Create a cache with default configuration
    pub fn with_defaults() -> Self {
        Self::new(StatementCacheConfig::default())
    }

    /// Get the cache configuration
    pub fn config(&self) -> &StatementCacheConfig {
        &self.config
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
            insertions: self.stats.insertions.load(Ordering::Relaxed),
        }
    }

    /// Look up a cached statement entry
    ///
    /// Returns the entry if found and updates access tracking.
    pub fn get(&self, sql: &str) -> Option<CacheEntry> {
        if !self.config.enabled {
            return None;
        }

        // First try read-only access
        let entries = self.entries.read().ok()?;

        if let Some(entry) = entries.get(sql) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            // Clone the entry before updating (to avoid holding read lock during write)
            let mut cloned = entry.clone();
            cloned.access_count = self.access_counter.fetch_add(1, Ordering::Relaxed);
            cloned.last_access = cloned.access_count;
            drop(entries);

            // Update the entry with new access time
            if let Ok(mut write_entries) = self.entries.write() {
                if let Some(e) = write_entries.get_mut(sql) {
                    e.access_count = cloned.access_count;
                    e.last_access = cloned.last_access;
                }
            }

            Some(cloned)
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Insert a statement into the cache
    ///
    /// If the cache is at capacity, the least recently used entry is evicted.
    pub fn insert(&self, sql: &str, param_count: usize) {
        if !self.config.enabled {
            return;
        }

        let access = self.access_counter.fetch_add(1, Ordering::Relaxed);
        let entry = CacheEntry::new(sql.to_string(), param_count, access);
        let entry_size = entry.size_bytes();

        let mut entries = match self.entries.write() {
            Ok(e) => e,
            Err(_) => return, // Lock poisoned, skip insert
        };

        // Check if already exists
        if entries.contains_key(sql) {
            // Just update access time
            if let Some(e) = entries.get_mut(sql) {
                e.access_count = access;
                e.last_access = access;
            }
            return;
        }

        // Evict entries if at capacity
        while entries.len() >= self.config.max_entries {
            self.evict_lru(&mut entries);
        }

        // Evict entries if over byte limit
        if let Some(max_bytes) = self.config.max_bytes {
            let mut current_bytes: usize = entries.values().map(|e| e.size_bytes()).sum();
            while current_bytes + entry_size > max_bytes && !entries.is_empty() {
                self.evict_lru(&mut entries);
                current_bytes = entries.values().map(|e| e.size_bytes()).sum();
            }
        }

        entries.insert(sql.to_string(), entry);
        self.stats.insertions.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if a statement is cached
    pub fn contains(&self, sql: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        self.entries
            .read()
            .map(|e| e.contains_key(sql))
            .unwrap_or(false)
    }

    /// Get the current number of cached entries
    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }

    /// Evict the least recently used entry
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry>) {
        if entries.is_empty() {
            return;
        }

        // Find the entry with the lowest last_access
        let lru_key = entries
            .iter()
            .min_by_key(|(_, e)| e.last_access)
            .map(|(k, _)| k.clone());

        if let Some(key) = lru_key {
            entries.remove(&key);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// Implement Debug manually to avoid issues with RwLock
impl std::fmt::Debug for StatementCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatementCache")
            .field("config", &self.config)
            .field("len", &self.len())
            .field("stats", &self.stats())
            .finish()
    }
}

/// Internal statistics tracking
#[derive(Default)]
struct CacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    insertions: AtomicU64,
}

/// Snapshot of cache statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStatsSnapshot {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of entries evicted
    pub evictions: u64,
    /// Number of entries inserted
    pub insertions: u64,
}

impl CacheStatsSnapshot {
    /// Get the cache hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statement_cache_config_default() {
        let config = StatementCacheConfig::default();
        assert_eq!(config.max_entries(), DEFAULT_MAX_ENTRIES);
        assert_eq!(config.max_bytes(), Some(DEFAULT_MAX_BYTES));
        assert!(config.enabled());
    }

    #[test]
    fn test_statement_cache_config_disabled() {
        let config = StatementCacheConfig::disabled();
        assert!(!config.enabled());
    }

    #[test]
    fn test_statement_cache_config_builder() {
        let config = StatementCacheConfig::builder()
            .max_entries(50)
            .max_bytes(1024)
            .enabled(true)
            .build();

        assert_eq!(config.max_entries(), 50);
        assert_eq!(config.max_bytes(), Some(1024));
        assert!(config.enabled());
    }

    #[test]
    fn test_statement_cache_config_no_byte_limit() {
        let config = StatementCacheConfig::builder().no_byte_limit().build();

        assert!(config.max_bytes().is_none());
    }

    #[test]
    fn test_statement_cache_insert_and_get() {
        let cache = StatementCache::with_defaults();

        cache.insert("SELECT * FROM users", 0);

        let entry = cache.get("SELECT * FROM users");
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.sql, "SELECT * FROM users");
        assert_eq!(entry.param_count, 0);
    }

    #[test]
    fn test_statement_cache_miss() {
        let cache = StatementCache::with_defaults();

        let entry = cache.get("SELECT * FROM nonexistent");
        assert!(entry.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_statement_cache_hit_tracking() {
        let cache = StatementCache::with_defaults();

        cache.insert("SELECT 1", 0);
        cache.get("SELECT 1");
        cache.get("SELECT 1");

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.insertions, 1);
    }

    #[test]
    fn test_statement_cache_lru_eviction() {
        let config = StatementCacheConfig::builder()
            .max_entries(2)
            .no_byte_limit()
            .build();
        let cache = StatementCache::new(config);

        cache.insert("SELECT 1", 0);
        cache.insert("SELECT 2", 0);

        // Access SELECT 1 to make it more recently used
        cache.get("SELECT 1");

        // Insert SELECT 3 - should evict SELECT 2 (LRU)
        cache.insert("SELECT 3", 0);

        assert!(cache.contains("SELECT 1"));
        assert!(!cache.contains("SELECT 2"));
        assert!(cache.contains("SELECT 3"));

        let stats = cache.stats();
        assert_eq!(stats.evictions, 1);
    }

    #[test]
    fn test_statement_cache_disabled() {
        let config = StatementCacheConfig::disabled();
        let cache = StatementCache::new(config);

        cache.insert("SELECT 1", 0);

        assert!(cache.get("SELECT 1").is_none());
        assert!(!cache.contains("SELECT 1"));
    }

    #[test]
    fn test_statement_cache_clear() {
        let cache = StatementCache::with_defaults();

        cache.insert("SELECT 1", 0);
        cache.insert("SELECT 2", 0);

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_stats_snapshot_hit_rate() {
        let stats = CacheStatsSnapshot {
            hits: 3,
            misses: 1,
            evictions: 0,
            insertions: 0,
        };

        assert!((stats.hit_rate() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cache_stats_snapshot_hit_rate_zero() {
        let stats = CacheStatsSnapshot::default();
        assert!((stats.hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cache_entry_size_bytes() {
        let entry = CacheEntry::new("SELECT * FROM users".to_string(), 0, 0);
        assert!(entry.size_bytes() > "SELECT * FROM users".len());
    }

    #[test]
    fn test_statement_cache_update_existing() {
        let cache = StatementCache::with_defaults();

        cache.insert("SELECT 1", 0);
        cache.insert("SELECT 1", 1); // Same SQL, different param count

        // Should only have one entry
        assert_eq!(cache.len(), 1);

        let stats = cache.stats();
        assert_eq!(stats.insertions, 1); // Only counted once
    }
}
