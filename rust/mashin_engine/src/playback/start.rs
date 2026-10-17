//! Presentation-only start requests never mutate the native audio or MA queue state.

use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::queue::PlayState;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

const START_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct PlaybackStartRequest {
    presentation: Arc<Mutex<PlaybackStart>>,
    id: u64,
}

impl PlaybackStartRequest {
    // A request retains its original backend across target switches and late responses.
    pub(crate) fn begin(
        presentation: Arc<Mutex<PlaybackStart>>,
        kind: StartKind,
        baseline: StartObservation,
    ) -> Self {
        let id = presentation
            .lock()
            .expect("playback start lock poisoned")
            .begin(kind, baseline);
        Self { presentation, id }
    }

    pub(crate) fn complete(&self, has_succeeded: bool) {
        self.presentation
            .lock()
            .expect("playback start lock poisoned")
            .complete(self.id, has_succeeded);
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.presentation
            .lock()
            .expect("playback start lock poisoned")
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == self.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartKind {
    Media,
    Resume,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StartIdentity {
    Local(Option<Uuid>),
    Remote(Option<QueueItemId>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartObservation {
    pub(crate) identity: StartIdentity,
    pub(crate) revision: u64,
    pub(crate) state: PlayState,
}

struct PendingStart {
    id: u64,
    kind: StartKind,
    baseline: StartObservation,
    started_at: Instant,
    has_completed_command: bool,
    has_observed_buffering: bool,
}

#[derive(Default)]
pub(crate) struct PlaybackStart {
    sequence: u64,
    pending: Option<PendingStart>,
    has_failed: bool,
}

impl PlaybackStart {
    // Start requests and command completion are matched independently of audio state.
    pub(crate) fn begin(&mut self, kind: StartKind, baseline: StartObservation) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.has_failed = false;
        self.pending = Some(PendingStart {
            id: self.sequence,
            kind,
            baseline,
            started_at: Instant::now(),
            has_completed_command: false,
            has_observed_buffering: false,
        });
        self.sequence
    }

    pub(crate) fn complete(&mut self, id: u64, has_succeeded: bool) {
        if let Some(pending) = self.pending.as_mut().filter(|pending| pending.id == id) {
            if has_succeeded {
                pending.has_completed_command = true;
            } else {
                self.pending = None;
                self.has_failed = true;
            }
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.pending = None;
        self.has_failed = false;
    }

    // Observations can end only their own start; old playing/error snapshots cannot.
    pub(crate) fn project(&mut self, observation: StartObservation, now: Instant) -> PlayState {
        let Some(pending) = self.pending.as_mut() else {
            return if self.has_failed {
                PlayState::Error
            } else {
                observation.state
            };
        };
        if now.saturating_duration_since(pending.started_at) >= START_TIMEOUT {
            self.pending = None;
            self.has_failed = true;
            return PlayState::Error;
        }
        let has_new_identity = observation.identity != pending.baseline.identity;
        let has_new_revision = observation.revision > pending.baseline.revision;
        if matches!(observation.identity, StartIdentity::Remote(_))
            && observation.state == PlayState::Buffering
            && pending.baseline.state != PlayState::Buffering
            && pending.has_completed_command
        {
            pending.has_observed_buffering = true;
        }
        let belongs_to_start = has_new_identity
            || (pending.kind == StartKind::Resume && has_new_revision)
            || (pending.kind == StartKind::Resume
                && observation.state == PlayState::Playing
                && pending.baseline.state != PlayState::Error)
            || pending.has_observed_buffering;
        if pending.has_completed_command
            && belongs_to_start
            && matches!(observation.state, PlayState::Playing | PlayState::Error)
        {
            self.pending = None;
            return observation.state;
        }
        PlayState::Buffering
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(identity: &str, revision: u64, state: PlayState) -> StartObservation {
        StartObservation {
            identity: StartIdentity::Remote(Some(QueueItemId(identity.into()))),
            revision,
            state,
        }
    }

    #[test]
    fn media_waits_for_new_audio_not_command_ack_or_old_playing() {
        let mut start = PlaybackStart::default();
        let old = observation("old", 1, PlayState::Playing);
        let request = start.begin(StartKind::Media, old.clone());
        assert_eq!(
            start.project(old.clone(), Instant::now()),
            PlayState::Buffering
        );
        start.complete(request, true);
        assert_eq!(start.project(old, Instant::now()), PlayState::Buffering);
        assert_eq!(
            start.project(observation("new", 0, PlayState::Playing), Instant::now()),
            PlayState::Playing
        );
    }

    #[test]
    fn resume_accepts_new_revision_but_not_the_old_error() {
        let mut start = PlaybackStart::default();
        let old = observation("stream", 3, PlayState::Error);
        let request = start.begin(StartKind::Resume, old.clone());
        assert_eq!(start.project(old, Instant::now()), PlayState::Buffering);
        start.complete(request, true);
        assert_eq!(
            start.project(observation("stream", 4, PlayState::Playing), Instant::now()),
            PlayState::Playing
        );
    }

    #[test]
    fn stale_completion_cannot_clear_new_start_and_cancel_restores_real_state() {
        let mut start = PlaybackStart::default();
        let old = observation("stream", 0, PlayState::Paused);
        let first = start.begin(StartKind::Media, old.clone());
        let second = start.begin(StartKind::Resume, old.clone());
        start.complete(first, false);
        assert_eq!(
            start.project(old.clone(), Instant::now()),
            PlayState::Buffering
        );
        start.complete(second, false);
        assert_eq!(start.project(old.clone(), Instant::now()), PlayState::Error);
        start.begin(StartKind::Resume, old.clone());
        start.cancel();
        assert_eq!(start.project(old, Instant::now()), PlayState::Paused);
    }

    #[test]
    fn matching_audio_error_and_timeout_end_buffering() {
        let mut start = PlaybackStart::default();
        let old = observation("old", 0, PlayState::Idle);
        let request = start.begin(StartKind::Media, old.clone());
        start.complete(request, true);
        assert_eq!(
            start.project(observation("new", 0, PlayState::Error), Instant::now()),
            PlayState::Error
        );
        start.begin(StartKind::Media, old);
        assert_eq!(
            start.project(
                observation("old", 0, PlayState::Idle),
                Instant::now() + START_TIMEOUT
            ),
            PlayState::Error
        );
    }

    #[test]
    fn cancelled_or_replaced_request_cannot_complete_its_successor() {
        let presentation = Arc::new(Mutex::new(PlaybackStart::default()));
        let old = observation("old", 0, PlayState::Paused);
        let first =
            PlaybackStartRequest::begin(presentation.clone(), StartKind::Media, old.clone());
        presentation.lock().unwrap().cancel();
        assert!(!first.is_pending());
        let second =
            PlaybackStartRequest::begin(presentation.clone(), StartKind::Resume, old.clone());
        first.complete(false);
        assert!(second.is_pending());
        assert_eq!(
            presentation.lock().unwrap().project(old, Instant::now()),
            PlayState::Buffering
        );
    }

    #[test]
    fn old_local_buffering_must_not_complete_a_media_start() {
        let mut start = PlaybackStart::default();
        let mut old = observation("old", 0, PlayState::Buffering);
        old.identity = StartIdentity::Local(Some(Uuid::new_v4()));
        let request = start.begin(StartKind::Media, old.clone());
        start.complete(request, true);
        assert_eq!(
            start.project(old.clone(), Instant::now()),
            PlayState::Buffering
        );
        old.revision += 1;
        old.state = PlayState::Playing;
        assert_eq!(start.project(old, Instant::now()), PlayState::Buffering);
    }

    #[test]
    fn cached_resume_has_no_minimum_buffering_duration() {
        let mut start = PlaybackStart::default();
        let old = observation("same", 0, PlayState::Paused);
        let request = start.begin(StartKind::Resume, old);
        start.complete(request, true);
        assert_eq!(
            start.project(observation("same", 0, PlayState::Playing), Instant::now()),
            PlayState::Playing
        );
    }
}
