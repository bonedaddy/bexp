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

const MAX_ENTRIES: usize = 100;
const TTL_SECS: u64 = 300; // 5 minutes

pub struct CapsuleCache {
    entries: RwLock<HashMap<CacheKey, CacheEntry>>,
}

impl Default for CapsuleCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CapsuleCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
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
        let entry = entries.get(&key)?;
        if entry.created_at.elapsed().as_secs() < TTL_SECS {
            Some(entry.result.clone())
        } else {
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
        entries.retain(|_, v| v.created_at.elapsed().as_secs() < TTL_SECS);

        // LRU-style eviction: remove oldest if at capacity
        if entries.len() >= MAX_ENTRIES {
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
