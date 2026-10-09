//! Tracks heard playback time using monotonic elapsed time and authoritative playback identities.

#[derive(Default)]
pub(super) struct ListenTracker {
    playback_id: String,
    previous_position_millis: i64,
    was_playing: bool,
    heard_millis: u64,
    has_submitted: bool,
    pub(super) started_at: i64,
}

impl ListenTracker {
    // Playback observations and suspension.
    pub(super) fn observe(
        &mut self,
        playback_id: &str,
        position_millis: i64,
        duration_millis: i64,
        is_playing: bool,
        elapsed_millis: u64,
        timestamp: i64,
    ) -> bool {
        if self.playback_id != playback_id {
            *self = Self {
                playback_id: playback_id.to_string(),
                previous_position_millis: position_millis,
                was_playing: is_playing,
                started_at: timestamp,
                ..Self::default()
            };
            return false;
        }
        let progress = position_millis.saturating_sub(self.previous_position_millis);
        if is_playing
            && self.was_playing
            && progress > 0
            && progress as u64 <= elapsed_millis.saturating_add(500)
            && elapsed_millis <= 5_000
        {
            self.heard_millis = self
                .heard_millis
                .saturating_add((progress as u64).min(elapsed_millis));
        }
        self.previous_position_millis = position_millis;
        self.was_playing = is_playing;
        let threshold = if duration_millis > 0 {
            (duration_millis as u64 / 2).min(240_000)
        } else {
            240_000
        };
        if !self.has_submitted && !playback_id.is_empty() && self.heard_millis >= threshold {
            self.has_submitted = true;
            return true;
        }
        false
    }

    pub(super) fn suspend(&mut self) {
        self.was_playing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submits_once_after_half_the_track() {
        let mut tracker = ListenTracker::default();
        assert!(!tracker.observe("one", 0, 10_000, true, 0, 100));
        for position in 1..5 {
            assert!(!tracker.observe("one", position * 1_000, 10_000, true, 1_000, 100 + position));
        }
        assert!(tracker.observe("one", 5_000, 10_000, true, 1_000, 105));
        assert!(!tracker.observe("one", 6_000, 10_000, true, 1_000, 106));
        assert_eq!(tracker.started_at, 100);
    }

    #[test]
    fn pauses_and_seeks_do_not_count_as_listening() {
        let mut tracker = ListenTracker::default();
        tracker.observe("one", 0, 100_000, true, 0, 100);
        tracker.observe("one", 1_000, 100_000, true, 1_000, 101);
        tracker.observe("one", 1_000, 100_000, false, 60_000, 161);
        tracker.observe("one", 90_000, 100_000, true, 1_000, 162);
        assert!(!tracker.observe("one", 91_000, 100_000, true, 1_000, 163));
        assert_eq!(tracker.heard_millis, 2_000);
    }

    #[test]
    fn long_tracks_use_four_minutes_and_repeats_start_a_new_listen() {
        let mut tracker = ListenTracker::default();
        tracker.observe("one", 0, 600_000, true, 0, 100);
        for second in 1..240 {
            assert!(!tracker.observe("one", second * 1_000, 600_000, true, 1_000, 100 + second));
        }
        assert!(tracker.observe("one", 240_000, 600_000, true, 1_000, 340));
        assert!(!tracker.observe("two", 0, 600_000, true, 1_000, 701));
        assert!(!tracker.has_submitted);
        assert_eq!(tracker.started_at, 701);
    }

    #[test]
    fn backward_seek_does_not_create_a_second_listen() {
        let mut tracker = ListenTracker::default();
        tracker.observe("one", 0, 4_000, true, 0, 100);
        tracker.observe("one", 1_000, 4_000, true, 1_000, 101);
        assert!(tracker.observe("one", 2_000, 4_000, true, 1_000, 102));
        tracker.observe("one", 3_000, 4_000, true, 1_000, 103);
        assert!(!tracker.observe("one", 0, 4_000, true, 1_000, 104));
        assert!(!tracker.observe("one", 2_000, 4_000, true, 2_000, 106));
    }
}
