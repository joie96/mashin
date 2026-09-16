use ::sendspin::audio::AudioFormat;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

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
    pub(crate) warm_keepalive_cancel: Arc<AtomicBool>,
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
    HardLimitReached,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CacheRetentionPolicy {
    pub(crate) current: Option<String>,
    pub(crate) protected: Vec<String>,
    pub(crate) prefetch: Vec<String>,
    pub(crate) recently_played: Vec<String>,
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

// Cache writes and completion.
pub(crate) fn cache_append_encoded_chunk(
    caches: &EncodedCaches,
    cache_sequence: &Arc<AtomicU64>,
    cache_capacity_bytes: usize,
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
                return CacheAppendResult::Appended;
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
        let incoming_bytes = data.len();
        let cached_bytes = caches
            .values()
            .fold(0_usize, |total, cache| total.saturating_add(cache.byte_len));
        if cached_bytes.saturating_add(incoming_bytes) > cache_capacity_bytes {
            return CacheAppendResult::HardLimitReached;
        }

        let cache = caches
            .entry(reservation.cache_content_key.clone())
            .or_insert_with(|| EncodedTrackCache {
                chunks: VecDeque::new(),
                byte_len: 0,
                cached_duration_millis: reservation.resume_after_millis,
                last_touched: touch,
                format: Some(format.clone()),
                integrity: CacheIntegrity::Partial,
            });
        cache.last_touched = touch;
        cache.format = Some(format.clone());
        cache.append(Arc::from(data), duration_millis);
        return CacheAppendResult::Appended;
    }

    CacheAppendResult::HardLimitReached
}

pub(crate) fn cache_mark_complete(caches: &EncodedCaches, reservation: &DownloadSlotReservation) {
    if let Ok(mut caches) = caches.lock() {
        if let Some(cache) = caches.get_mut(&reservation.cache_content_key) {
            cache.integrity = CacheIntegrity::Complete;
        }
    }
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
    if let Ok(mut caches) = caches.lock() {
        caches.remove(content_key);
    }
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

pub(crate) fn cache_discard_interrupted_outside_window(
    caches: &EncodedCaches,
    content_keys: &[String],
) -> Vec<String> {
    let Ok(mut caches) = caches.lock() else {
        return Vec::new();
    };
    let discarded = caches
        .iter()
        .filter(|(key, cache)| {
            cache.integrity == CacheIntegrity::Interrupted && !content_keys.contains(key)
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in &discarded {
        caches.remove(key);
    }
    discarded
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

pub(crate) fn cache_read_chunk(
    caches: &EncodedCaches,
    content_key: &str,
    chunk_index: usize,
) -> Result<(Option<Arc<[u8]>>, bool), String> {
    let mut caches = caches
        .lock()
        .map_err(|_| "encoded cache lock poisoned".to_string())?;
    let cache = caches
        .get_mut(content_key)
        .ok_or_else(|| "encoded track cache is unavailable".to_string())?;
    cache.last_touched = cache.last_touched.saturating_add(1);
    Ok((
        cache
            .chunks
            .get(chunk_index)
            .map(|chunk| Arc::clone(&chunk.data)),
        cache.integrity == CacheIntegrity::Complete,
    ))
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
    Ok((
        cache
            .chunks
            .get(chunk_index)
            .map(|chunk| (Arc::clone(&chunk.data), chunk.duration_millis)),
        cache.integrity == CacheIntegrity::Complete,
    ))
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
        chunk_index,
        chunk_start_position_millis: chunk.start_position_millis,
        skip_within_chunk_millis: target_position_millis
            .saturating_sub(chunk.start_position_millis),
    })
}

// Cache eviction policy.
pub(crate) fn cache_evict_to_fit(
    caches: &EncodedCaches,
    policy: &CacheRetentionPolicy,
    cache_capacity_bytes: usize,
    incoming_bytes: usize,
) -> Vec<String> {
    let Ok(mut caches) = caches.lock() else {
        return Vec::new();
    };
    let mut evicted = Vec::new();
    while caches
        // Cache metadata queries.
        .values()
        .map(|cache| cache.byte_len)
        .sum::<usize>()
        .saturating_add(incoming_bytes)
        > cache_capacity_bytes
    {
        let candidate = caches
            .iter()
            .filter(|(key, _)| {
                policy.current.as_deref() != Some(key.as_str())
                    && !policy.protected.iter().any(|value| value == *key)
            })
            .min_by_key(|(key, cache)| eviction_rank(key, cache, policy))
            .map(|(key, _)| key.clone());
        let Some(key) = candidate else {
            break;
        };
        caches.remove(&key);
        evicted.push(key);
    }
    evicted
}

fn eviction_rank(
    cache_content_key: &str,
    cache: &EncodedTrackCache,
    policy: &CacheRetentionPolicy,
) -> (u8, i64) {
    if policy
        .recently_played
        .iter()
        .any(|key| key == cache_content_key)
    {
        return (30, cache.last_touched as i64);
    }
    let Some(index) = policy
        .prefetch
        .iter()
        .position(|key| key == cache_content_key)
    else {
        return (0, cache.last_touched as i64);
    };
    (20, -(index as i64))
}

pub(crate) fn cache_complete_duration_millis(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<u64> {
    let caches = caches.lock().ok()?;
    let cache = caches.get(content_key)?;
    (cache.integrity == CacheIntegrity::Complete).then_some(cache.cached_duration_millis)
}

pub(crate) fn cache_total_bytes(caches: &EncodedCaches) -> usize {
    caches
        .lock()
        .map(|caches| caches.values().map(|cache| cache.byte_len).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reservation(cache_content_key: &str, lease_id: &str) -> DownloadSlotReservation {
        DownloadSlotReservation {
            cache_content_key: cache_content_key.to_string(),
            expected_duration_millis: None,
            resume_after_millis: 0,
            append_after_seek: Arc::new(AtomicBool::new(true)),
            lease_id: lease_id.to_string(),
            warm_keepalive_cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn reservation_is_only_current_for_its_slot_and_lease() {
        let first = reservation("provider:track:first", "slot-1-1");
        let slots: DownloadSlots = Arc::new(Mutex::new([Some(first.clone()), None]));

        assert!(slot_reservation_is_current(&slots, 0, &first));
        assert!(!slot_reservation_is_current(&slots, 1, &first));
        assert!(!slot_reservation_is_current(
            &slots,
            0,
            &reservation("provider:track:first", "slot-1-2"),
        ));
    }

    #[test]
    fn release_reservation_only_removes_the_matching_lease() {
        let first = reservation("provider:track:first", "slot-1-1");
        let replacement = reservation("provider:track:second", "slot-1-2");
        let slots: DownloadSlots = Arc::new(Mutex::new([Some(first.clone()), None]));

        assert!(slot_release_reservation_if_current(&slots, 0, &first));
        assert!(slots.lock().expect("slot lock")[0].is_none());

        slots.lock().expect("slot lock")[0] = Some(replacement.clone());
        assert!(!slot_release_reservation_if_current(&slots, 0, &first));
        assert_eq!(
            slots.lock().expect("slot lock")[0]
                .as_ref()
                .map(|value| value.lease_id.as_str()),
            Some("slot-1-2"),
        );
    }

    #[test]
    fn preserves_warm_caches_when_the_hard_limit_is_reached() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let first = reservation("provider:track:first", "slot-1-1");
        let second = reservation("provider:track:second", "slot-2-1");

        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        assert_eq!(
            cache_append_encoded_chunk(&caches, &sequence, 10, &first, &format, 10, &[1; 6]),
            CacheAppendResult::Appended,
        );
        assert_eq!(
            cache_append_encoded_chunk(&caches, &sequence, 10, &second, &format, 10, &[2; 6]),
            CacheAppendResult::HardLimitReached,
        );

        let caches = caches.lock().expect("cache lock");
        assert!(caches.contains_key(&first.cache_content_key));
        assert!(!caches.contains_key(&second.cache_content_key));
    }

    #[test]
    fn stores_chunks_until_the_ram_cache_limit() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let reservation = reservation("provider:track:soft-limit", "slot-1-1");
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };

        assert_eq!(
            cache_append_encoded_chunk(
                &caches,
                &sequence,
                100,
                &reservation,
                &format,
                90,
                &[1; 90]
            ),
            CacheAppendResult::Appended,
        );
        let (chunk, _) = cache_read_chunk(&caches, &reservation.cache_content_key, 0)
            .expect("stored soft-limit chunk");
        assert_eq!(chunk.expect("first chunk").len(), 90);
    }

    #[test]
    fn evicts_untracked_then_prefetch_without_touching_protected_tracks() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        for key in ["past", "current", "next", "future", "orphan"] {
            let reservation = reservation(&format!("provider:track:{key}"), "slot-1-1");
            cache_append_encoded_chunk(
                &caches,
                &sequence,
                100,
                &reservation,
                &format,
                10,
                &[1; 10],
            );
        }
        let policy = CacheRetentionPolicy {
            current: Some("provider:track:current".to_string()),
            protected: vec![
                "provider:track:current".to_string(),
                "provider:track:next".to_string(),
            ],
            prefetch: vec!["provider:track:future".to_string()],
            recently_played: vec!["provider:track:past".to_string()],
        };
        assert_eq!(
            cache_evict_to_fit(&caches, &policy, 40, 0),
            vec!["provider:track:orphan"]
        );
        assert_eq!(
            cache_evict_to_fit(&caches, &policy, 30, 0),
            vec!["provider:track:future"]
        );
        let caches = caches.lock().expect("cache lock");
        assert!(caches.contains_key("provider:track:past"));
        assert!(caches.contains_key("provider:track:current"));
        assert!(caches.contains_key("provider:track:next"));
    }

    #[test]
    fn replaces_partial_cache_when_the_negotiated_format_changes() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let reservation = reservation("provider:track:format", "slot-1-1");
        let first_format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        let second_format = AudioFormat {
            sample_rate: 44_100,
            ..first_format.clone()
        };

        cache_append_encoded_chunk(
            &caches,
            &sequence,
            100,
            &reservation,
            &first_format,
            10,
            &[1],
        );
        cache_append_encoded_chunk(
            &caches,
            &sequence,
            100,
            &reservation,
            &second_format,
            10,
            &[2],
        );

        assert_eq!(
            cache_read_format(&caches, &reservation.cache_content_key)
                .expect("replacement format")
                .sample_rate,
            44_100,
        );
        let (chunk, _) = cache_read_chunk(&caches, &reservation.cache_content_key, 0)
            .expect("replacement chunk");
        assert_eq!(chunk.expect("first chunk").as_ref(), &[2]);
    }

    #[test]
    fn partial_cache_exposes_received_chunks_before_stream_end() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let reservation = reservation("provider:track:partial", "slot-1-1");
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };

        cache_append_encoded_chunk(&caches, &sequence, 100, &reservation, &format, 10, &[1, 2]);

        let (chunk, complete) = cache_read_chunk(&caches, &reservation.cache_content_key, 0)
            .expect("partial cache should be readable");
        assert_eq!(chunk.expect("first chunk").as_ref(), &[1, 2]);
        assert!(!complete);
    }

    #[test]
    fn discarding_incomplete_cache_keeps_completed_cache() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let partial = reservation("provider:track:partial", "slot-1-1");
        let complete = reservation("provider:track:complete", "slot-2-1");
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        cache_append_encoded_chunk(&caches, &sequence, 100, &partial, &format, 10, &[1]);
        cache_append_encoded_chunk(&caches, &sequence, 100, &complete, &format, 10, &[2]);
        cache_mark_complete(&caches, &complete);

        cache_discard_incomplete(&caches, &partial.cache_content_key);
        cache_discard_incomplete(&caches, &complete.cache_content_key);

        let caches = caches.lock().expect("cache lock");
        assert!(!caches.contains_key(&partial.cache_content_key));
        assert!(caches.contains_key(&complete.cache_content_key));
    }

    #[test]
    fn discards_only_interrupted_caches_that_leave_the_download_window() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        let interrupted = reservation("provider:track:interrupted", "slot-1-1");
        let healthy = reservation("provider:track:healthy", "slot-2-1");
        cache_append_encoded_chunk(&caches, &sequence, 100, &interrupted, &format, 10, &[1]);
        cache_append_encoded_chunk(&caches, &sequence, 100, &healthy, &format, 10, &[2]);
        assert_eq!(
            cache_mark_interrupted(&caches, &interrupted.cache_content_key),
            Some(10)
        );

        assert_eq!(
            cache_discard_interrupted_outside_window(
                &caches,
                std::slice::from_ref(&healthy.cache_content_key),
            ),
            vec![interrupted.cache_content_key.clone()]
        );
        let caches = caches.lock().expect("cache lock");
        assert!(!caches.contains_key(&interrupted.cache_content_key));
        assert!(caches.contains_key(&healthy.cache_content_key));
    }
}
