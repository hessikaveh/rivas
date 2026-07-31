use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Decoded image data, either a single PNG frame or an animated GIF.
#[derive(Clone)]
pub enum ImageData {
    /// A single PNG frame with raw bytes, width, and height.
    Png(Vec<u8>, u32, u32),
    /// An animated GIF with frames (each frame has raw bytes and delay in ms), width, and height.
    Gif {
        frames: Vec<(Vec<u8>, u32)>,
        width: u32,
        height: u32,
    },
}

impl ImageData {
    /// Returns the display width in pixels.
    pub fn width(&self) -> u32 {
        match self {
            ImageData::Png(_, w, _) => *w,
            ImageData::Gif { width, .. } => *width,
        }
    }

    /// Returns the display height in pixels.
    pub fn height(&self) -> u32 {
        match self {
            ImageData::Png(_, _, h) => *h,
            ImageData::Gif { height, .. } => *height,
        }
    }
}

struct AssetEntry {
    image: ImageData,
}

/// Thread-safe in-memory cache for decoded image data.
///
/// Bounded to 64 entries with simple half-eviction when full.
pub struct AssetCache {
    cache: Arc<Mutex<HashMap<u64, AssetEntry>>>,
}

impl AssetCache {
    /// Creates an empty asset cache.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Retrieves a cached image by its hash key, if present.
    pub fn get(&self, key: u64) -> Option<ImageData> {
        let cache = self.cache.lock().ok()?;
        cache.get(&key).map(|e| e.image.clone())
    }

    /// Stores an image in the cache under the given hash key.
    ///
    /// Evicts half the cache when it exceeds 64 entries.
    pub fn insert(&self, key: u64, image: ImageData) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, AssetEntry { image });
            if cache.len() > 64 {
                let to_remove = cache.len() / 2;
                let keys: Vec<u64> = cache.keys().copied().collect();
                for key in keys.iter().take(to_remove) {
                    cache.remove(key);
                }
            }
        }
    }
}
