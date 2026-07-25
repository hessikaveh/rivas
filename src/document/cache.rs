use super::model::Document;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Simple content hash for cache invalidation
fn compute_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Thread-safe parse cache
pub struct ParseCache {
    cache: Arc<Mutex<HashMap<u64, Document>>>,
}

impl ParseCache {
    pub fn new() -> Self {
        ParseCache {
            cache: Arc::new(Mutex::new(HashMap::with_capacity(16))),
        }
    }

    /// Get cached document or return None if not cached
    pub fn get(&self, content: &str) -> Option<Document> {
        let hash = compute_hash(content);
        let cache = self.cache.lock().unwrap();
        cache.get(&hash).cloned()
    }

    /// Store parsed document in cache
    pub fn insert(&self, content: &str, doc: Document) {
        let hash = compute_hash(content);
        let mut cache = self.cache.lock().unwrap();
        cache.insert(hash, doc);

        // Keep cache size bounded to avoid unbounded memory growth
        if cache.len() > 32 {
            // Remove oldest entries (simple strategy: clear half)
            let to_remove = cache.len() / 2;
            let keys: Vec<u64> = cache.keys().copied().collect();
            for key in keys.iter().take(to_remove) {
                cache.remove(key);
            }
        }
    }
}

impl Clone for ParseCache {
    fn clone(&self) -> Self {
        ParseCache {
            cache: Arc::clone(&self.cache),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let cache = ParseCache::new();
        let content = "# Test";
        let doc1 = Document { blocks: vec![] };

        cache.insert(content, doc1.clone());

        let doc2 = cache.get(content);
        assert!(doc2.is_some());
    }

    #[test]
    fn test_cache_miss() {
        let cache = ParseCache::new();
        let result = cache.get("# Different content");
        assert!(result.is_none());
    }

    #[test]
    fn test_different_content_different_hash() {
        let cache = ParseCache::new();
        let doc1 = Document { blocks: vec![] };
        let doc2 = Document { blocks: vec![] };

        cache.insert("# A", doc1);
        cache.insert("# B", doc2);

        // Should get first doc for first content
        let result = cache.get("# A");
        assert!(result.is_some());
    }
}
