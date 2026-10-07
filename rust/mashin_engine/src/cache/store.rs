use crate::sendspin::reconnect::{PcmResumeReference, PcmTail};
use ::sendspin::audio::AudioFormat;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const CACHE_COMPLETION_TOLERANCE_MILLIS: u64 = 500;
static NEXT_CACHE_IDENTITY: AtomicU64 = AtomicU64::new(1);

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
    pub(crate) title: String,
    pub(crate) expected_duration_millis: Option<u64>,
    pub(crate) resume_after_millis: u64,
    pub(crate) resume_reference: Option<Arc<PcmResumeReference>>,
    pub(crate) lease_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheIntegrity {
    Partial,
    Interrupted,
    Complete,
}

#[derive(Clone)]
pub(crate) struct EncodedCacheChunk {
    pub(crate) data: Arc<[u8]>,
    pub(crate) start_frame: u64,
    pub(crate) frame_count: u64,
    pub(crate) leading_frames_to_skip: u64,
    pub(crate) sample_rate: u32,
}

impl EncodedCacheChunk {
    // Audible timeline queries.
    pub(crate) fn audible_frames(&self) -> u64 {
        self.frame_count.saturating_sub(self.leading_frames_to_skip)
    }

    pub(crate) fn duration_millis(&self) -> u64 {
        self.audible_frames().saturating_mul(1_000) / u64::from(self.sample_rate.max(1))
    }

    fn start_position_millis(&self) -> u64 {
        self.start_frame.saturating_mul(1_000) / u64::from(self.sample_rate.max(1))
    }
}

pub(crate) struct EncodedTrackCache {
    chunks: VecDeque<EncodedCacheChunk>,
    first_chunk_index: usize,
    byte_len: usize,
    cached_duration_millis: u64,
    last_touched: u64,
    format: Option<AudioFormat>,
    integrity: CacheIntegrity,
    identity: u64,
    end_frame: u64,
    pcm_tail: Option<PcmTail>,
}

impl EncodedTrackCache {
    fn append(&mut self, chunk: EncodedCacheChunk) {
        self.byte_len = self.byte_len.saturating_add(chunk.data.len());
        self.end_frame = chunk.start_frame.saturating_add(chunk.audible_frames());
        self.cached_duration_millis =
            self.end_frame.saturating_mul(1_000) / u64::from(chunk.sample_rate.max(1));
        self.chunks.push_back(chunk);
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
    pub(crate) skip_within_chunk_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheAppendResult {
    Appended,
    Completed,
    IgnoredComplete,
    IgnoredConnectionLost,
    IgnoredOverlap,
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
                    identity: NEXT_CACHE_IDENTITY.fetch_add(1, Ordering::Relaxed),
                    end_frame: 0,
                    pcm_tail: None,
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

pub(crate) fn cache_item_counts(caches: &EncodedCaches, content_keys: &[String]) -> (usize, usize) {
    let Ok(caches) = caches.lock() else {
        return (0, 0);
    };
    content_keys
        .iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .fold((0, 0), |(completed, partly), key| {
            match caches.get(key).filter(|cache| cache.byte_len > 0) {
                Some(cache) if cache.integrity == CacheIntegrity::Complete => {
                    (completed + 1, partly)
                }
                Some(_) => (completed, partly + 1),
                None => (completed, partly),
            }
        })
}

// Appends an encoded chunk only to an explicitly reserved cache key and marks completed streams.
pub(crate) fn cache_append_encoded_chunk(
    caches: &EncodedCaches,
    cache_sequence: &Arc<AtomicU64>,
    reservation: &DownloadSlotReservation,
    format: &AudioFormat,
    chunk: EncodedCacheChunk,
    decoded_samples: &[i32],
) -> CacheAppendResult {
    if chunk.data.is_empty() || chunk.audible_frames() == 0 {
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
                return CacheAppendResult::WriteUnavailable;
            }
        }
        let Some(cache) = caches.get_mut(&reservation.cache_content_key) else {
            return CacheAppendResult::IgnoredStaleLease;
        };
        if reservation
            .resume_reference
            .as_ref()
            .is_some_and(|reference| reference.cache_identity != cache.identity)
        {
            return CacheAppendResult::IgnoredStaleLease;
        }
        if !cache.chunks.is_empty() && chunk.start_frame != cache.end_frame {
            return CacheAppendResult::IgnoredOverlap;
        }
        cache.last_touched = touch;
        cache.format = Some(format.clone());
        let tail = cache
            .pcm_tail
            .get_or_insert_with(|| PcmTail::new(format.sample_rate, format.channels));
        let skip_samples =
            chunk.leading_frames_to_skip as usize * usize::from(format.channels.max(1));
        if let Some(samples) = decoded_samples.get(skip_samples..) {
            tail.append(samples);
        }
        cache.integrity = CacheIntegrity::Partial;
        cache.append(chunk);
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

// Immutable restart snapshots; packet decoding and matching never hold the cache lock.
pub(crate) fn cache_resume_reference(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<Arc<PcmResumeReference>> {
    let caches = caches.lock().ok()?;
    let cache = caches.get(content_key)?;
    let format = cache.format.as_ref()?;
    let samples = cache.pcm_tail.as_ref()?.reference(format.sample_rate);
    if samples.is_empty() {
        return None;
    }
    Some(Arc::new(PcmResumeReference {
        cache_identity: cache.identity,
        end_frame: cache.end_frame,
        format: format.clone(),
        samples,
    }))
}

pub(crate) fn cache_identity(caches: &EncodedCaches, content_key: &str) -> Option<u64> {
    caches
        .lock()
        .ok()?
        .get(content_key)
        .map(|cache| cache.identity)
}

pub(crate) fn cache_audio_duration_millis(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<u64> {
    let caches = caches.lock().ok()?;
    let cache = caches.get(content_key)?;
    let sample_rate = u64::from(cache.format.as_ref()?.sample_rate.max(1));
    Some(
        cache
            .chunks
            .iter()
            .map(EncodedCacheChunk::audible_frames)
            .sum::<u64>()
            * 1_000
            / sample_rate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(start_millis: u64, duration_millis: u64, data: &[u8]) -> EncodedCacheChunk {
        EncodedCacheChunk {
            data: Arc::from(data),
            start_frame: start_millis * 48,
            frame_count: duration_millis * 48,
            leading_frames_to_skip: 0,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn old_identity_cannot_read_a_rebuilt_cache() {
        let caches = Arc::new(Mutex::new(HashMap::new()));
        cache_add_key(&caches, "track".to_string(), 0);
        let identity = cache_identity(&caches, "track").unwrap();
        cache_discard(&caches, "track");
        cache_add_key(&caches, "track".to_string(), 0);
        assert_ne!(cache_identity(&caches, "track"), Some(identity));
        let (chunk, is_complete) = cache_read_chunk(&caches, "track", 0, Some(identity)).unwrap();
        assert!(chunk.is_none());
        assert!(!is_complete);
    }

    #[test]
    fn counts_only_distinct_cached_items_with_audio() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        for (key, expected_duration_millis) in [("partial", 2_000), ("complete", 1_000)] {
            cache_add_key(&caches, key.to_string(), 0);
            let reservation = DownloadSlotReservation {
                cache_content_key: key.to_string(),
                title: "Track".to_string(),
                expected_duration_millis: Some(expected_duration_millis),
                resume_after_millis: 0,
                resume_reference: None,
                lease_id: key.to_string(),
            };
            cache_append_encoded_chunk(
                &caches,
                &sequence,
                &reservation,
                &format,
                chunk(0, 1_000, b"audio"),
                &[],
            );
        }
        cache_add_key(&caches, "empty".to_string(), 1_000);
        let keys = ["partial", "complete", "empty", "missing", "partial"].map(str::to_string);

        assert_eq!(cache_item_counts(&caches, &keys), (1, 1));
        cache_remove_key(&caches, "partial");
        assert_eq!(cache_item_counts(&caches, &keys), (1, 0));
    }

    #[test]
    fn resume_keeps_indexes_and_trims_the_transition_sample_exactly() {
        let caches: EncodedCaches = Arc::new(Mutex::new(HashMap::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let reservation = DownloadSlotReservation {
            cache_content_key: "track".to_string(),
            title: "Track".to_string(),
            expected_duration_millis: Some(6_000),
            resume_after_millis: 2_000,
            resume_reference: None,
            lease_id: "slot-1-1".to_string(),
        };
        let format = AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        cache_add_key(&caches, "track".to_string(), 0);
        for (start, duration, data) in [
            (0, 1_500, b"a".as_slice()),
            (1_500, 1_000, b"b"),
            (2_500, 1_000, b"c"),
        ] {
            assert_eq!(
                cache_append_encoded_chunk(
                    &caches,
                    &sequence,
                    &reservation,
                    &format,
                    chunk(start, duration, data),
                    &[],
                ),
                CacheAppendResult::Appended
            );
        }
        assert_eq!(cache_mark_interrupted(&caches, "track"), Some(3_500));
        assert_eq!(
            cache_append_encoded_chunk(
                &caches,
                &sequence,
                &reservation,
                &format,
                chunk(2_000, 1_000, b"duplicate"),
                &[],
            ),
            CacheAppendResult::IgnoredOverlap
        );
        assert_eq!(cache_cached_duration_millis(&caches, "track"), Some(3_500));
        assert_eq!(
            cache_append_encoded_chunk(
                &caches,
                &sequence,
                &reservation,
                &format,
                EncodedCacheChunk {
                    start_frame: 168_000,
                    leading_frames_to_skip: 24_000,
                    ..chunk(3_000, 1_000, b"new")
                },
                &[],
            ),
            CacheAppendResult::Appended
        );
        assert_eq!(cache_cached_duration_millis(&caches, "track"), Some(4_000));
        let transition = cache_read_chunk(&caches, "track", 3, None)
            .unwrap()
            .0
            .unwrap();
        assert_eq!(transition.leading_frames_to_skip, 24_000);
        assert_eq!(transition.audible_frames(), 24_000);
        assert_eq!(
            cache_local_seek_target(&caches, "track", 3_750)
                .unwrap()
                .skip_within_chunk_frames,
            12_000
        );
        assert_eq!(cache_audio_duration_millis(&caches, "track"), Some(4_000));
        assert_eq!(
            cache_local_seek_target(&caches, "track", 3_750)
                .unwrap()
                .chunk_index,
            3
        );
        assert_eq!(
            cache_read_chunk(&caches, "track", 3, None)
                .unwrap()
                .0
                .unwrap()
                .data
                .as_ref(),
            b"new"
        );
        assert_eq!(
            cache_append_encoded_chunk(
                &caches,
                &sequence,
                &reservation,
                &format,
                chunk(4_000, 2_000, b"end"),
                &[],
            ),
            CacheAppendResult::Completed
        );
        assert_eq!(
            cache_complete_duration_millis(&caches, "track"),
            Some(6_000)
        );
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

pub(crate) fn cache_read_chunk(
    caches: &EncodedCaches,
    content_key: &str,
    chunk_index: usize,
    expected_identity: Option<u64>,
) -> Result<(Option<EncodedCacheChunk>, bool), String> {
    let mut caches = caches
        .lock()
        .map_err(|_| "encoded cache lock poisoned".to_string())?;
    let cache = caches
        .get_mut(content_key)
        .ok_or_else(|| "encoded track cache is unavailable".to_string())?;
    if expected_identity.is_some_and(|identity| identity != cache.identity) {
        return Ok((None, false));
    }
    cache.last_touched = cache.last_touched.saturating_add(1);
    let deque_index = chunk_index.checked_sub(cache.first_chunk_index);
    Ok((
        deque_index.and_then(|index| cache.chunks.get(index).cloned()),
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
                    .start_position_millis()
                    .saturating_add(chunk.duration_millis())
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
        let target_frame = target_position_millis * u64::from(chunk.sample_rate) / 1_000;
        target_frame >= chunk.start_frame
            && target_frame < chunk.start_frame + chunk.audible_frames()
    })?;
    cache.last_touched = cache.last_touched.saturating_add(1);
    Some(LocalCacheSeekTarget {
        chunk_index: cache.first_chunk_index.saturating_add(chunk_index),
        chunk_start_position_millis: chunk.start_position_millis(),
        skip_within_chunk_frames: target_position_millis
            .saturating_mul(u64::from(chunk.sample_rate))
            .saturating_div(1_000)
            .saturating_sub(chunk.start_frame),
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
