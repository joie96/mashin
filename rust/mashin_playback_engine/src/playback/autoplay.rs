//! Queue-based autoplay policy independent of playback engine implementations.

use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::media::Track;
use serde_json::json;
use std::collections::HashSet;

const AUTOPLAY_REFILL_THRESHOLD: usize = 5;
const AUTOPLAY_BATCH_SIZE: usize = 25;
const AUTOPLAY_MAXIMUM_SEED_TRACKS: usize = 10;
const AUTOPLAY_BASE_SAMPLE_SIZE: usize = 5;
const AUTOPLAY_MAXIMUM_TRACK_DURATION_SECONDS: u32 = 20 * 60;

pub(crate) struct AutoplayController {
    is_enabled: bool,
    is_refill_in_progress: bool,
    seed_tracks: Vec<Track>,
}

impl AutoplayController {
    // Construction and configuration.
    pub(crate) fn new() -> Self {
        Self {
            is_enabled: false,
            is_refill_in_progress: false,
            seed_tracks: Vec::new(),
        }
    }

    pub(crate) fn set_enabled(&mut self, is_enabled: bool) {
        self.is_enabled = is_enabled;
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    // Seed management follows explicit user queue actions, never generated autoplay tracks.
    pub(crate) fn replace_seed_tracks(&mut self, seed_tracks: Vec<Track>) {
        self.seed_tracks.clear();
        self.append_seed_tracks(seed_tracks);
    }

    pub(crate) fn append_seed_tracks(&mut self, seed_tracks: Vec<Track>) {
        self.seed_tracks.extend(seed_tracks);
        let excess = self
            .seed_tracks
            .len()
            .saturating_sub(AUTOPLAY_MAXIMUM_SEED_TRACKS);
        if excess > 0 {
            self.seed_tracks.drain(..excess);
        }
    }

    // Refill reservation prevents concurrent workers from scheduling the same queue extension.
    pub(crate) fn reserve_refill(
        &mut self,
        queue_length: usize,
        current_index: Option<usize>,
    ) -> Option<Vec<Track>> {
        if !self.is_enabled || self.is_refill_in_progress || self.seed_tracks.is_empty() {
            return None;
        }
        let current_index = current_index?;
        let upcoming_count = queue_length.saturating_sub(current_index.saturating_add(1));
        if upcoming_count >= AUTOPLAY_REFILL_THRESHOLD {
            return None;
        }
        self.is_refill_in_progress = true;
        Some(self.seed_tracks.clone())
    }

    pub(crate) fn finish_refill(&mut self) {
        self.is_refill_in_progress = false;
    }
}

impl Default for AutoplayController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::AutoplayController;
    use crate::music_assistant::models::media::Track;

    fn track(index: usize) -> Track {
        serde_json::from_value(serde_json::json!({
            "media_type": "track",
            "item_id": format!("track-{index}"),
            "provider": "library",
            "name": format!("Track {index}"),
        }))
        .expect("deserialize track")
    }

    #[test]
    fn reserves_one_refill_when_the_queue_has_few_upcoming_items() {
        let mut autoplay = AutoplayController::new();
        autoplay.set_enabled(true);
        autoplay.replace_seed_tracks(vec![track(1)]);

        assert_eq!(
            autoplay
                .reserve_refill(6, Some(1))
                .map(|tracks| tracks.len()),
            Some(1)
        );
        assert!(autoplay.reserve_refill(6, Some(1)).is_none());

        autoplay.finish_refill();

        assert!(autoplay.reserve_refill(6, Some(1)).is_some());
    }

    #[test]
    fn retains_only_the_most_recent_seed_tracks() {
        let mut autoplay = AutoplayController::new();
        autoplay.append_seed_tracks((0..12).map(track).collect());

        let tracks = autoplay.reserve_refill(1, Some(0));

        assert!(tracks.is_none());
        autoplay.set_enabled(true);
        let tracks = autoplay.reserve_refill(1, Some(0)).expect("reserve refill");
        assert_eq!(tracks.len(), 10);
        assert_eq!(
            tracks.first().expect("first seed").media.item_id.as_str(),
            "track-2"
        );
    }
}

// Music Assistant candidate resolution stays outside the coordinator lock.
pub(crate) async fn get_autoplay_refill_tracks(
    client: &MusicAssistantClient,
    seed_tracks: Vec<Track>,
    queued_tracks: Vec<Track>,
) -> Result<Vec<Track>, String> {
    let sampled_seed_tracks = seed_tracks
        .into_iter()
        .take(AUTOPLAY_BASE_SAMPLE_SIZE)
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for seed_track in &sampled_seed_tracks {
        let arguments = json!({
            "limit": AUTOPLAY_BATCH_SIZE,
            "allow_lookup": false,
        });
        let mut similar_tracks = client
            .get_similar_tracks(
                &seed_track.media.item_id,
                seed_track.media.provider.as_str(),
                arguments,
            )
            .await?;
        if similar_tracks.is_empty() {
            similar_tracks = client
                .get_similar_tracks(
                    &seed_track.media.item_id,
                    seed_track.media.provider.as_str(),
                    json!({
                        "limit": AUTOPLAY_BATCH_SIZE,
                        "allow_lookup": true,
                    }),
                )
                .await?;
        }
        candidates.extend(similar_tracks);
    }
    let excluded_track_keys = queued_tracks
        .iter()
        .chain(sampled_seed_tracks.iter())
        .map(track_key)
        .collect::<HashSet<_>>();
    let mut candidate_track_keys = HashSet::new();
    Ok(candidates
        .into_iter()
        .filter(|track| track.duration <= AUTOPLAY_MAXIMUM_TRACK_DURATION_SECONDS)
        .filter(|track| !excluded_track_keys.contains(&track_key(track)))
        .filter(|track| candidate_track_keys.insert(track_key(track)))
        .take(AUTOPLAY_BATCH_SIZE)
        .collect())
}

fn track_key(track: &Track) -> String {
    track
        .media
        .uri
        .as_ref()
        .map(|uri| uri.as_str())
        .filter(|uri| !uri.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                track.media.provider.as_str(),
                track.media.item_id.as_str()
            )
        })
}
