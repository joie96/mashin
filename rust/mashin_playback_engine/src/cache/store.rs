use ::sendspin::audio::AudioFormat;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub(crate) type CacheChangeSignal = Arc<(Mutex<u64>, Condvar)>;

pub(crate) fn new_cache_change_signal() -> CacheChangeSignal {
    Arc::new((Mutex::new(0), Condvar::new()))
}

pub(crate) fn notify_cache_changed(signal: &CacheChangeSignal) {
    if let Ok(mut generation) = signal.0.lock() {
        *generation = generation.saturating_add(1);
        signal.1.notify_all();
    }
}

pub(crate) fn wait_for_cache_change(signal: &CacheChangeSignal, generation: u64) -> u64 {
    let Ok(mut current) = signal.0.lock() else {
        return generation;
    };
    while *current == generation {
        current = signal
            .1
            .wait(current)
            .unwrap_or_else(|error| error.into_inner());
    }
    *current
}

#[derive(Debug, Clone)]
pub(crate) struct DownloadSlotReservation {
    pub(crate) cache_content_key: String,
    pub(crate) lease_id: String,
    pub(crate) warm_keepalive_cancel: Arc<AtomicBool>,
}

pub(crate) struct EncodedTrackCache {
    chunks: VecDeque<Arc<[u8]>>,
    byte_len: usize,
    cached_duration_millis: u64,
    last_touched: u64,
    format: Option<AudioFormat>,
    complete: bool,
}

impl EncodedTrackCache {
    fn append(&mut self, chunk: Arc<[u8]>, duration_millis: u64) {
        self.byte_len = self.byte_len.saturating_add(chunk.len());
        self.cached_duration_millis = self.cached_duration_millis.saturating_add(duration_millis);
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
pub(crate) enum CacheAppendResult {
    Appended,
    SoftLimitReached,
    HardLimitReached,
}

pub(crate) fn reservation_is_current(
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

pub(crate) fn cache_encoded_chunk(
    caches: &EncodedCaches,
    cache_sequence: &Arc<AtomicU64>,
    cache_capacity_bytes: usize,
    cache_pause_threshold_bytes: usize,
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
            if existing.complete {
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
                cached_duration_millis: 0,
                last_touched: touch,
                format: Some(format.clone()),
                complete: false,
            });
        cache.last_touched = touch;
        cache.format = Some(format.clone());
        cache.append(Arc::from(data), duration_millis);
        if cached_bytes.saturating_add(incoming_bytes) >= cache_pause_threshold_bytes {
            return CacheAppendResult::SoftLimitReached;
        }
        return CacheAppendResult::Appended;
    }

    CacheAppendResult::HardLimitReached
}

pub(crate) fn mark_cache_complete(caches: &EncodedCaches, reservation: &DownloadSlotReservation) {
    if let Ok(mut caches) = caches.lock() {
        if let Some(cache) = caches.get_mut(&reservation.cache_content_key) {
            cache.complete = true;
        }
    }
}

pub(crate) fn discard_incomplete_cache(caches: &EncodedCaches, content_key: &str) {
    if let Ok(mut caches) = caches.lock() {
        let remove = caches
            .get(content_key)
            .map(|cache| !cache.complete)
            .unwrap_or(false);
        if remove {
            caches.remove(content_key);
        }
    }
}

pub(crate) fn discard_all_incomplete_caches(caches: &EncodedCaches) {
    if let Ok(mut caches) = caches.lock() {
        caches.retain(|_, cache| cache.complete);
    }
}

pub(crate) fn read_cache_format(
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

pub(crate) fn read_cache_chunk(
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
    Ok((cache.chunks.get(chunk_index).cloned(), cache.complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reservation(cache_content_key: &str, lease_id: &str) -> DownloadSlotReservation {
        DownloadSlotReservation {
            cache_content_key: cache_content_key.to_string(),
            lease_id: lease_id.to_string(),
            warm_keepalive_cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn reservation_is_only_current_for_its_slot_and_lease() {
        let first = reservation("provider:track:first", "slot-1-1");
        let slots: DownloadSlots = Arc::new(Mutex::new([Some(first.clone()), None]));

        assert!(reservation_is_current(&slots, 0, &first));
        assert!(!reservation_is_current(&slots, 1, &first));
        assert!(!reservation_is_current(
            &slots,
            0,
            &reservation("provider:track:first", "slot-1-2"),
        ));
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
            cache_encoded_chunk(&caches, &sequence, 10, 9, &first, &format, 10, &[1; 6]),
            CacheAppendResult::Appended,
        );
        assert_eq!(
            cache_encoded_chunk(&caches, &sequence, 10, 9, &second, &format, 10, &[2; 6]),
            CacheAppendResult::HardLimitReached,
        );

        let caches = caches.lock().expect("cache lock");
        assert!(caches.contains_key(&first.cache_content_key));
        assert!(!caches.contains_key(&second.cache_content_key));
    }

    #[test]
    fn reports_soft_limit_after_storing_the_chunk() {
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
            cache_encoded_chunk(
                &caches,
                &sequence,
                100,
                90,
                &reservation,
                &format,
                90,
                &[1; 90]
            ),
            CacheAppendResult::SoftLimitReached,
        );
        let (chunk, _) = read_cache_chunk(&caches, &reservation.cache_content_key, 0)
            .expect("stored soft-limit chunk");
        assert_eq!(chunk.expect("first chunk").len(), 90);
    }

    #[test]
    fn evicts_by_queue_position_without_touching_current_or_next() {
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
            cache_encoded_chunk(
                &caches,
                &sequence,
                100,
                90,
                &reservation,
                &format,
                10,
                &[1; 10],
            );
        }
        let plan = CachePlan {
            current_cache_content_key: Some("provider:track:current".to_string()),
            next_cache_content_key: Some("provider:track:next".to_string()),
            last_played_cache_content_key: Some("provider:track:past".to_string()),
            ordered_cache_content_keys: vec![
                "provider:track:past".to_string(),
                "provider:track:current".to_string(),
                "provider:track:next".to_string(),
                "provider:track:future".to_string(),
            ],
        };

        assert_eq!(
            evict_to_target(&caches, &plan, 40),
            vec!["provider:track:orphan"]
        );
        assert_eq!(
            evict_to_target(&caches, &plan, 30),
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

        cache_encoded_chunk(
            &caches,
            &sequence,
            100,
            90,
            &reservation,
            &first_format,
            10,
            &[1],
        );
        cache_encoded_chunk(
            &caches,
            &sequence,
            100,
            90,
            &reservation,
            &second_format,
            10,
            &[2],
        );

        assert_eq!(
            read_cache_format(&caches, &reservation.cache_content_key)
                .expect("replacement format")
                .sample_rate,
            44_100,
        );
        let (chunk, _) = read_cache_chunk(&caches, &reservation.cache_content_key, 0)
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

        cache_encoded_chunk(
            &caches,
            &sequence,
            100,
            90,
            &reservation,
            &format,
            10,
            &[1, 2],
        );

        let (chunk, complete) = read_cache_chunk(&caches, &reservation.cache_content_key, 0)
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
        cache_encoded_chunk(&caches, &sequence, 100, 90, &partial, &format, 10, &[1]);
        cache_encoded_chunk(&caches, &sequence, 100, 90, &complete, &format, 10, &[2]);
        mark_cache_complete(&caches, &complete);

        discard_incomplete_cache(&caches, &partial.cache_content_key);
        discard_incomplete_cache(&caches, &complete.cache_content_key);

        let caches = caches.lock().expect("cache lock");
        assert!(!caches.contains_key(&partial.cache_content_key));
        assert!(caches.contains_key(&complete.cache_content_key));
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachePlan {
    pub(crate) current_cache_content_key: Option<String>,
    pub(crate) next_cache_content_key: Option<String>,
    pub(crate) last_played_cache_content_key: Option<String>,
    pub(crate) ordered_cache_content_keys: Vec<String>,
}

pub(crate) fn evict_to_target(
    caches: &EncodedCaches,
    plan: &CachePlan,
    target_bytes: usize,
) -> Vec<String> {
    let Ok(mut caches) = caches.lock() else {
        return Vec::new();
    };
    let mut evicted = Vec::new();
    while caches.values().map(|cache| cache.byte_len).sum::<usize>() > target_bytes {
        let candidate = caches
            .keys()
            .min_by_key(|key| eviction_rank(key, plan))
            .cloned();
        let Some(key) = candidate else {
            break;
        };
        if eviction_rank(&key, plan).0 >= 100 {
            break;
        }
        caches.remove(&key);
        evicted.push(key);
    }
    evicted
}

fn eviction_rank(cache_content_key: &str, plan: &CachePlan) -> (u8, i64) {
    if plan.current_cache_content_key.as_deref() == Some(cache_content_key) {
        return (100, 0);
    }
    if plan.next_cache_content_key.as_deref() == Some(cache_content_key) {
        return (90, 0);
    }
    if plan.last_played_cache_content_key.as_deref() == Some(cache_content_key) {
        return (80, 0);
    }
    let Some(index) = plan
        .ordered_cache_content_keys
        .iter()
        .position(|key| key == cache_content_key)
    else {
        return (0, 0);
    };
    let current_index = plan.current_cache_content_key.as_ref().and_then(|current| {
        plan.ordered_cache_content_keys
            .iter()
            .position(|key| key == current)
    });
    match current_index {
        Some(current_index) if index < current_index => (10, index as i64),
        Some(_) => (20, -(index as i64)),
        None => (20, -(index as i64)),
    }
}

pub(crate) fn cache_byte_len(caches: &EncodedCaches) -> usize {
    caches
        .lock()
        .map(|caches| caches.values().map(|cache| cache.byte_len).sum())
        .unwrap_or(0)
}

pub(crate) fn complete_cache_duration_millis(
    caches: &EncodedCaches,
    content_key: &str,
) -> Option<u64> {
    let caches = caches.lock().ok()?;
    let cache = caches.get(content_key)?;
    cache.complete.then_some(cache.cached_duration_millis)
}
