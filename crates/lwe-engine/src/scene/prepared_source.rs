//! Weak CPU caches only retain identities, never source pixels or budget leases.
//! Callers hash bytes from a fresh descriptor read before querying these caches.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use lwe_scene::{AnimationMetadata, AtlasMemoryReservation, TimedAtlasSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StaticTextureStorage {
    Rgba8,
    Rg8,
    R8,
}

#[derive(Debug)]
pub(crate) struct PreparedStaticTexture {
    pub fingerprint: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub format: u32,
    pub flags: u32,
    pub animation: Option<AnimationMetadata>,
    pub(super) storage: StaticTextureStorage,
    rgba: Vec<u8>,
    // Pixel and frame storage drops before its reservation.
    reservation: AtlasMemoryReservation,
}
impl PreparedStaticTexture {
    pub(super) fn new(
        decoded: lwe_scene::Texture,
        fingerprint: [u8; 32],
        reservation: AtlasMemoryReservation,
        raw_mip: bool,
    ) -> Self {
        let storage = match (
            raw_mip
                && decoded.metadata.container_format == -1
                && decoded.metadata.animation.is_none(),
            decoded.metadata.format,
        ) {
            (true, 8) => StaticTextureStorage::Rg8,
            (true, 9) => StaticTextureStorage::R8,
            _ => StaticTextureStorage::Rgba8,
        };
        Self {
            fingerprint,
            width: decoded.width,
            height: decoded.height,
            image_width: decoded.metadata.image_width,
            image_height: decoded.metadata.image_height,
            format: decoded.metadata.format,
            flags: decoded.metadata.flags,
            animation: decoded.metadata.animation,
            storage,
            rgba: decoded.rgba,
            reservation,
        }
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    pub fn capacity_bytes(&self) -> usize {
        self.reservation.bytes()
    }
}
const MAX_ENTRIES: usize = 1024;
type Cache<T> = Mutex<BTreeMap<[u8; 32], Weak<T>>>;
fn static_cache() -> &'static Cache<PreparedStaticTexture> {
    static CACHE: OnceLock<Cache<PreparedStaticTexture>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
fn timed_cache() -> &'static Cache<TimedAtlasSource> {
    static CACHE: OnceLock<Cache<TimedAtlasSource>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
fn get<T>(cache: &Cache<T>, key: [u8; 32]) -> Option<Arc<T>> {
    let mut cache = cache.lock().unwrap_or_else(|poison| poison.into_inner());
    cache.retain(|_, value| value.strong_count() != 0);
    cache.get(&key).and_then(Weak::upgrade)
}
fn insert<T>(cache: &Cache<T>, key: [u8; 32], value: &Arc<T>) {
    let mut cache = cache.lock().unwrap_or_else(|poison| poison.into_inner());
    cache.retain(|_, value| value.strong_count() != 0);
    if cache.len() >= MAX_ENTRIES {
        cache.pop_first();
    }
    cache.insert(key, Arc::downgrade(value));
}
pub(super) fn get_static(key: [u8; 32]) -> Option<Arc<PreparedStaticTexture>> {
    get(static_cache(), key)
}
pub(super) fn put_static(key: [u8; 32], value: &Arc<PreparedStaticTexture>) {
    insert(static_cache(), key, value)
}
pub(super) fn get_timed(key: [u8; 32]) -> Option<Arc<TimedAtlasSource>> {
    get(timed_cache(), key)
}
pub(super) fn put_timed(key: [u8; 32], value: &Arc<TimedAtlasSource>) {
    insert(timed_cache(), key, value)
}
