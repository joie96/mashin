//! Presentation-only start requests never mutate the native audio or MA queue state.

use crate::domain::identifiers::QueueItemId;
use crate::playback::models::PlayState;
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
