use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

/// Cache key for capsule results.
#[derive(Hash, Eq, PartialEq, Clone)]
struct CacheKey {
    query: String,
    token_budget: usize,
    intent: String,
    index_generation: u64,
}

struct CacheEntry {
    result: String,
    created_at: Instant,
}

const DEFAULT_MAX_ENTRIES: usize = 100;
const DEFAULT_TTL_SECS: u64 = 300; // 5 minutes

pub struct CapsuleCache {
    entries: RwLock<HashMap<CacheKey, CacheEntry>>,
    max_entries: usize,
    ttl_secs: u64,
}

impl Default for CapsuleCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES, DEFAULT_TTL_SECS)
    }
}

impl CapsuleCache {
    pub fn new(max_entries: usize, ttl_secs: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_entries,
            ttl_secs,
        }
    }

    /// Try to get a cached result. Returns None on miss or expired entry.
    pub fn get(
        &self,
        query: &str,
        token_budget: usize,
        intent: &str,
        index_generation: u64,
    ) -> Option<String> {
        let key = Self::make_key(query, token_budget, intent, index_generation);
        let entries = self.entries.read().ok()?;
        let entry = match entries.get(&key) {
            Some(e) => e,
            None => {
                crate::metrics::record_cache_miss();
                return None;
            }
        };
        if entry.created_at.elapsed().as_secs() < self.ttl_secs {
            crate::metrics::record_cache_hit();
            Some(entry.result.clone())
        } else {
            crate::metrics::record_cache_miss();
            None
        }
    }

    /// Store a result in the cache. Evicts oldest entries if at capacity.
    pub fn put(
        &self,
        query: &str,
        token_budget: usize,
        intent: &str,
        index_generation: u64,
        result: String,
    ) {
        let key = Self::make_key(query, token_budget, intent, index_generation);
        let mut entries = match self.entries.write() {
            Ok(e) => e,
            Err(_) => return,
        };

        // Evict expired entries first
        entries.retain(|_, v| v.created_at.elapsed().as_secs() < self.ttl_secs);

        // LRU-style eviction: remove oldest if at capacity
        if entries.len() >= self.max_entries {
            if let Some(oldest_key) = entries
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest_key);
            }
        }

        entries.insert(
            key,
            CacheEntry {
                result,
                created_at: Instant::now(),
            },
        );
    }

    fn make_key(query: &str, token_budget: usize, intent: &str, index_generation: u64) -> CacheKey {
        CacheKey {
            query: query.to_string(),
            token_budget,
            intent: intent.to_string(),
            index_generation,
        }
    }
}
