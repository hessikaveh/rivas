use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// File list cache with TTL (Time-To-Live)
pub struct FileListCache {
    cached_files: Arc<Mutex<Option<CachedFileList>>>,
}

struct CachedFileList {
    files: Vec<String>,
    timestamp: u64,
}

impl FileListCache {
    pub fn new() -> Self {
        FileListCache {
            cached_files: Arc::new(Mutex::new(None)),
        }
    }

    /// Get cached file list if fresh (within TTL)
    pub fn get(&self, ttl_secs: u64) -> Option<Vec<String>> {
        let cache = self.cached_files.lock().unwrap();

        if let Some(cached) = cache.as_ref() {
            let now = current_time_secs();
            if now - cached.timestamp < ttl_secs {
                return Some(cached.files.clone());
            }
        }
        None
    }

    /// Store file list in cache
    pub fn set(&self, files: Vec<String>) {
        let mut cache = self.cached_files.lock().unwrap();
        *cache = Some(CachedFileList {
            files,
            timestamp: current_time_secs(),
        });
    }
}

impl Clone for FileListCache {
    fn clone(&self) -> Self {
        FileListCache {
            cached_files: Arc::clone(&self.cached_files),
        }
    }
}

fn current_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit_within_ttl() {
        let cache = FileListCache::new();
        let files = vec!["test.rs".to_string(), "main.rs".to_string()];

        cache.set(files.clone());
        let result = cache.get(60); // 60 second TTL

        assert!(result.is_some());
        assert_eq!(result.unwrap(), files);
    }

    #[test]
    fn test_empty_cache() {
        let cache = FileListCache::new();
        let result = cache.get(60);

        assert!(result.is_none());
    }
}
