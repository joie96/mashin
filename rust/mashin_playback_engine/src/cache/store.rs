use ::sendspin::audio::AudioFormat;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const CACHE_COMPLETION_TOLERANCE_MILLIS: u64 = 500;

pub(crate) type CacheChangeSignal = Arc<(Mutex<u64>, Condvar)>;

// Cache-change synchronization.
pub(crate) fn new_cache_change_signal() -> CacheChangeSignal {
    Arc::new((Mutex::new(0), Condvar::new()))
}

pub(crate) fn notify_cache_changed(signal: &CacheChangeSignal) {
    if let Ok(mut generation) = signal.0.lock() {
        *generation = generation.saturating_add(1);
        signal.1.notify_all();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DownloadSlotReservation {
    pub(crate) cache_content_key: String,
    pub(crate) expected_duration_millis: Option<u64>,
    pub(crate) resume_after_millis: u64,
    pub(crate) append_after_seek: Arc<AtomicBool>,
    pub(crate) lease_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheIntegrity {
    Partial,
    Interrupted,
    Complete,
}

struct EncodedCacheChunk {
    data: Arc<[u8]>,
    start_position_millis: u64,
    duration_millis: u64,
}

pub(crate) struct EncodedTrackCache {
    chunks: VecDeque<EncodedCacheChunk>,
    first_chunk_index: usize,
    byte_len: usize,
    cached_duration_millis: u64,
    last_touched: u64,
    format: Option<AudioFormat>,
    integrity: CacheIntegrity,
}

impl EncodedTrackCache {
    fn append(&mut self, chunk: Arc<[u8]>, duration_millis: u64) {
        self.byte_len = self.byte_len.saturating_add(chunk.len());
        let start_position_millis = self.cached_duration_millis;
        self.cached_duration_millis = self.cached_duration_millis.saturating_add(duration_millis);
        self.chunks.push_back(EncodedCacheChunk {
            data: chunk,
            start_position_millis,
            duration_millis,
        });
    }
}

fn audio_format_matches(left: &AudioFormat, right: &AudioFormat) -> bool {
    left.codec == right.codec
        && left.sample_rate == right.sample_rate
        && left.channels == right.channels
        && left.bit_depth == right.bit_depth
}

pub(crate) type DownloadSlots = Arc<Mutex<[Option<DownloadSlotReservation>; 2]>>;
pub(crate) type EncodedCaches = Arc<Mutex<HashMap<String, EncodedTrackCache>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalCacheSeekTarget {
    pub(crate) chunk_index: usize,
    pub(crate) chunk_start_position_millis: u64,
    pub(crate) skip_within_chunk_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheAppendResult {
    Appended,
    Completed,
    IgnoredComplete,
    IgnoredStaleLease,
    WriteUnavailable,
}

// Download-slot reservation management.
pub(crate) fn slot_reservation_is_current(
    slots: &DownloadSlots,
    slot_index: usize,
    reservation: &DownloadSlotReservation,
) -> bool {
    slots
        .lock()
        .ok()
        .and_then(|slots| slots.get(slot_index).cloned().flatten())
        .map(|current| current.lease_id == reservation.lease_id)
        .unwrap_or(false)
}

pub(crate) fn slot_release_reservation_if_current(
    slots: &DownloadSlots,
    slot_index: usize,
    reservation: &DownloadSlotReservation,
) -> bool {
    slots
        .lock()
        .map(|mut slots| {
            let Some(current) = slots[slot_index].as_ref() else {
                return false;
            };
            if current.lease_id != reservation.lease_id {
                return false;
            }
            slots[slot_index] = None;
            true
        })
        .unwrap_or(false)
}

// Cache key and chunk operations.
pub(crate) fn cache_add_key(
    caches: &EncodedCaches,
    content_key: String,
    resume_after_millis: u64,
) -> bool {
    caches
        .lock()
        .map(|mut caches| {
            caches
                .entry(content_key)
                .or_insert_with(|| EncodedTrackCache {
                    chunks: VecDeque::new(),
                    first_chunk_index: 0,
                    byte_len: 0,
                    cached_duration_millis: resume_after_millis,
                    last_touched: 0,
                    format: None,
                    integrity: CacheIntegrity::Partial,
                });
            true
        })
        .unwrap_or(false)
}

pub(crate) fn cache_remove_key(caches: &EncodedCaches, content_key: &str) -> bool {
    caches
        .lock()
        .map(|mut caches| caches.remove(content_key).is_some())
        .unwrap_or(false)
}

pub(crate) fn cache_keys(caches: &EncodedCaches) -> Vec<String> {
    caches
        .lock()
        .map(|caches| caches.keys().cloned().collect())
        .unwrap_or_default()
}

// Appends an encoded chunk only to an explicitly reserved cache key and marks completed streams.
pub(crate) fn cache_append_encoded_chunk(
    caches: &EncodedCaches,
    cache_sequence: &Arc<AtomicU64>,
    reservation: &DownloadSlotReservation,
    format: &AudioFormat,
    duration_millis: u64,
    data: &[u8],
) -> CacheAppendResult {
    if data.is_empty() {
        return CacheAppendResult::Appended;
    }

    if let Ok(mut caches) = caches.lock() {
        let touch = cache_sequence.fetch_add(1, Ordering::Relaxed);
        if let Some(existing) = caches.get(&reservation.cache_content_key) {
            if existing.integrity == CacheIntegrity::Complete {
                return CacheAppendResult::IgnoredComplete;
            }
            if existing
                .format
                .as_ref()
                .map(|existing_format| !audio_format_matches(existing_format, format))
                .unwrap_or(false)
            {
                caches.remove(&reservation.cache_content_key);
            }
        }
        let Some(cache) = caches.get_mut(&reservation.cache_content_key) else {
            return CacheAppendResult::IgnoredStaleLease;
        };
        cache.last_touched = touch;
        cache.format = Some(format.clone());
        cache.append(Arc::from(data), duration_millis);
        if reservation
            .expected_duration_millis
            .map(|expected| {
                cache
                    .cached_duration_millis
                    .saturating_add(CACHE_COMPLETION_TOLERANCE_MILLIS)
                    >= expected
            })
            .unwrap_or(false)
        {
            cache.integrity = CacheIntegrity::Complete;
            return CacheAppendResult::Completed;
        }
        return CacheAppendResult::Appended;
    }

    CacheAppendResult::WriteUnavailable
}

pub(crate) fn cache_discard_incomplete(caches: &EncodedCaches, content_key: &str) {
    if let Ok(mut caches) = caches.lock() {
        let remove = caches
            .get(content_key)
            .map(|cache| cache.integrity != CacheIntegrity::Complete)
            .unwrap_or(false);
        if remove {
            caches.remove(content_key);
        }
    }
}

pub(crate) fn cache_discard(caches: &EncodedCaches, content_key: &str) {
    let _ = cache_remove_key(caches, content_key);
}

pub(crate) fn cache_clear_all(caches: &EncodedCaches) {
    if let Ok(mut caches) = caches.lock() {
        caches.clear();
    }
}

// Interrupted-cache handling.
pub(crate) fn cache_mark_interrupted(caches: &EncodedCaches, content_key: &str) -> Option<u64> {
    let Ok(mut caches) = caches.lock() else {
        return None;
    };
    let cache = caches.get_mut(content_key)?;
    if cache.integrity == CacheIntegrity::Complete {
        return None;
    }
    cache.integrity = CacheIntegrity::Interrupted;
    Some(cache.cached_duration_millis)
}

pub(crate) fn cache_interrupted_duration_millis(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<u64> {
    let caches = caches.lock().ok()?;
    let cache = caches.get(content_key)?;
    (cache.integrity == CacheIntegrity::Interrupted).then_some(cache.cached_duration_millis)
}

// Retains the chunk that crosses the resume position without splitting encoded audio frames.
pub(crate) fn cache_truncate_after_millis(
    caches: &EncodedCaches,
    content_key: &str,
    resume_position_millis: u64,
) {
    if let Ok(mut caches) = caches.lock() {
        let Some(cache) = caches.get_mut(content_key) else {
            return;
        };
        if cache.integrity != CacheIntegrity::Interrupted {
            return;
        }
        while cache
            .chunks
            .back()
            .map(|chunk| chunk.start_position_millis >= resume_position_millis)
            .unwrap_or(false)
        {
            if let Some(chunk) = cache.chunks.pop_back() {
                cache.byte_len = cache.byte_len.saturating_sub(chunk.data.len());
            }
        }
        cache.cached_duration_millis = resume_position_millis;
    }
}

// Cache reading and local seeking.
pub(crate) fn cache_read_format(
    caches: &EncodedCaches,
    content_key: &str,
) -> Result<AudioFormat, String> {
    let mut caches = caches
        .lock()
        .map_err(|_| "encoded cache lock poisoned".to_string())?;
    let cache = caches
        .get_mut(content_key)
        .ok_or_else(|| "encoded track cache is unavailable".to_string())?;
    let format = cache
        .format
        .clone()
        .ok_or_else(|| "encoded track cache format is unavailable".to_string())?;
    cache.last_touched = cache.last_touched.saturating_add(1);
    Ok(format)
}

pub(crate) fn cache_read_chunk_with_duration(
    caches: &EncodedCaches,
    content_key: &str,
    chunk_index: usize,
) -> Result<(Option<(Arc<[u8]>, u64)>, bool), String> {
    let mut caches = caches
        .lock()
        .map_err(|_| "encoded cache lock poisoned".to_string())?;
    let cache = caches
        .get_mut(content_key)
        .ok_or_else(|| "encoded track cache is unavailable".to_string())?;
    cache.last_touched = cache.last_touched.saturating_add(1);
    let deque_index = chunk_index.checked_sub(cache.first_chunk_index);
    Ok((
        deque_index.and_then(|index| {
            cache
                .chunks
                .get(index)
                .map(|chunk| (Arc::clone(&chunk.data), chunk.duration_millis))
        }),
        cache.integrity == CacheIntegrity::Complete,
    ))
}

// Removes only chunks already played by the current item until the requested cache target fits.
pub(crate) fn cache_trim_played_current_to_capacity(
    caches: &EncodedCaches,
    content_key: &str,
    played_position_millis: u64,
    target_capacity_bytes: usize,
) -> usize {
    let Ok(mut caches) = caches.lock() else {
        return 0;
    };
    let total_bytes = caches.values().map(|cache| cache.byte_len).sum::<usize>();
    let Some(cache) = caches.get_mut(content_key) else {
        return 0;
    };
    let mut remaining_bytes = total_bytes;
    let mut removed_bytes: usize = 0;
    while remaining_bytes > target_capacity_bytes
        && cache
            .chunks
            .front()
            .map(|chunk| {
                chunk
                    .start_position_millis
                    .saturating_add(chunk.duration_millis)
                    <= played_position_millis
            })
            .unwrap_or(false)
    {
        let Some(chunk) = cache.chunks.pop_front() else {
            break;
        };
        cache.first_chunk_index = cache.first_chunk_index.saturating_add(1);
        cache.byte_len = cache.byte_len.saturating_sub(chunk.data.len());
        remaining_bytes = remaining_bytes.saturating_sub(chunk.data.len());
        removed_bytes = removed_bytes.saturating_add(chunk.data.len());
    }
    removed_bytes
}

pub(crate) fn cache_local_seek_target(
    caches: &EncodedCaches,
    content_key: &str,
    target_position_millis: u64,
) -> Option<LocalCacheSeekTarget> {
    let mut caches = caches.lock().ok()?;
    let cache = caches.get_mut(content_key)?;
    let (chunk_index, chunk) = cache.chunks.iter().enumerate().find(|(_, chunk)| {
        target_position_millis >= chunk.start_position_millis
            && target_position_millis
                <= chunk
                    .start_position_millis
                    .saturating_add(chunk.duration_millis)
    })?;
    cache.last_touched = cache.last_touched.saturating_add(1);
    Some(LocalCacheSeekTarget {
        chunk_index: cache.first_chunk_index.saturating_add(chunk_index),
        chunk_start_position_millis: chunk.start_position_millis,
        skip_within_chunk_millis: target_position_millis
            .saturating_sub(chunk.start_position_millis),
    })
}

pub(crate) fn cache_complete_duration_millis(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<u64> {
    let caches = caches.lock().ok()?;
    let cache = caches.get(content_key)?;
    (cache.integrity == CacheIntegrity::Complete).then_some(cache.cached_duration_millis)
}

pub(crate) fn cache_cached_duration_millis(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<u64> {
    caches.lock().ok().and_then(|caches| {
        caches
            .get(content_key)
            .map(|cache| cache.cached_duration_millis)
    })
}

pub(crate) fn cache_total_bytes(caches: &EncodedCaches) -> usize {
    caches
        .lock()
        .map(|caches| caches.values().map(|cache| cache.byte_len).sum())
        .unwrap_or(0)
}
