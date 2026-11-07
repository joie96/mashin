//! Rejected audio loses its cache, not the user's playback intent or audible position.

use super::audio_adapter::MashinAudioAdapter;
use super::audio_adapter::OutputSnapshot;
use super::models::{OutputState, PlaybackIdentity};
use super::protocol::ControlCommand;
use super::receiver::{ConnectionState, ReceiverSnapshot};
use crate::logging::Logger;
use crate::playback::server::QueueCommand;
use crate::playback::server::{PlaybackServer, PlaybackServerError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;
use uuid::Uuid;

const LOGGER: Logger = Logger::new("mashin");

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryTarget {
    pub(crate) playback: PlaybackIdentity,
    pub(crate) position_seconds: u64,
    pub(crate) is_paused: bool,
}

impl RecoveryTarget {
    pub(crate) fn from_snapshot(snapshot: &OutputSnapshot) -> Option<Self> {
        let stream = snapshot.current.as_ref()?;
        if !matches!(
            snapshot.resume.output_state,
            OutputState::Playing
                | OutputState::Buffering
                | OutputState::Paused
                | OutputState::Error
        ) {
            return None;
        }
        let position_millis = stream.media_start_position_millis.checked_add(
            snapshot.resume.position_frame.checked_mul(1000)?
                / u64::from(stream.output_format.sample_rate_hz),
        )?;
        Some(Self {
            playback: stream.playback.clone(),
            position_seconds: position_millis / 1000,
            is_paused: snapshot.resume.output_state == OutputState::Paused,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RecoveryState {
    pub(crate) revision: u64,
    pub(crate) retry_revision: u64,
    pub(crate) target: Option<RecoveryTarget>,
    pub(crate) replacement_stream_id: Option<Uuid>,
    pub(crate) is_stopped: bool,
    pub(crate) is_paused: Option<bool>,
}

impl RecoveryState {
    // User actions invalidate already captured asynchronous recovery requests immediately.
    pub(crate) fn cancel(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.retry_revision = self.retry_revision.wrapping_add(1);
        self.target = None;
        self.replacement_stream_id = None;
    }

    pub(crate) fn set_paused(&mut self, is_paused: bool) {
        self.is_paused = Some(is_paused);
        if let Some(target) = &mut self.target {
            target.is_paused = is_paused;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

pub(super) struct RecoveryBackoff {
    revision: u64,
    next_attempt: Instant,
    delay: Duration,
}

impl RecoveryBackoff {
    // Each unchanged intent retries forever, but never faster than its capped schedule.
    pub(super) fn new(revision: u64, now: Instant) -> Self {
        Self {
            revision,
            next_attempt: now,
            delay: Duration::from_secs(1),
        }
    }

    pub(super) fn is_due(&mut self, revision: u64, now: Instant) -> bool {
        if self.revision != revision {
            *self = Self::new(revision, now);
        }
        now >= self.next_attempt
    }

    pub(super) fn attempted(&mut self, now: Instant) {
        self.next_attempt = now + self.delay;
        self.delay = (self.delay * 2).min(Duration::from_secs(30));
    }
}

// The session owns this worker; no API request may outlive its captured local intent.
pub(super) async fn run(
    output: MashinAudioAdapter,
    client: Arc<dyn PlaybackServer>,
    mut connections: watch::Receiver<ReceiverSnapshot>,
) {
    let mut intent = output.recovery();
    let mut backoff = RecoveryBackoff::new(0, Instant::now());
    let mut ticks = tokio::time::interval(Duration::from_millis(250));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let state = intent.borrow_and_update().clone();
        let connection = connections.borrow_and_update().clone();
        let is_connected = connection.control_connection == ConnectionState::Connected
            && connection.audio_connection == ConnectionState::Connected;
        if let Some(target) = state.target.filter(|_| {
            is_connected
                && state.replacement_stream_id.is_none()
                && backoff.is_due(state.retry_revision, Instant::now())
        }) {
            backoff.attempted(Instant::now());
            let request = async {
                let queues = client.get_player_queues().await?;
                let queue = queues
                    .iter()
                    .find(|queue| queue.queue_id == target.playback.queue_id);
                if !queue
                    .and_then(|queue| queue.current_item.as_ref())
                    .is_some_and(|item| item.queue_item_id == target.playback.queue_item_id)
                {
                    output.cancel_recovery();
                    LOGGER.info("MA queue item changed; playback recovery canceled");
                    return Ok(());
                }
                if output.recovery().borrow().revision != state.revision {
                    return Ok(());
                }
                client
                    .execute_queue_command(
                        &target.playback.queue_id,
                        QueueCommand::PlayItem {
                            item_id: target.playback.queue_item_id.clone(),
                            position_seconds: target.position_seconds,
                        },
                    )
                    .await
            };
            LOGGER.info(format_args!(
                "Requesting replacement MA load: queue={} item={} position={}s paused={}",
                target.playback.queue_id.as_str(),
                target.playback.queue_item_id.as_str(),
                target.position_seconds,
                target.is_paused
            ));
            tokio::select! {
                biased;
                changed = intent.changed() => { if changed.is_err() { return; } }
                changed = connections.changed() => { if changed.is_err() { return; } }
                result = tokio::time::timeout(Duration::from_secs(30), request) => {
                    match result {
                        Ok(Ok(())) => LOGGER.info("Replacement MA request completed; waiting for authorized media"),
                        Ok(Err(PlaybackServerError::Denied(_))) => {
                            LOGGER.warn("Replacement MA request denied; stopping playback recovery without authentication retries");
                            let _ = output.execute(ControlCommand::Stop { command_id: 0, stream_id: None }).await;
                        }
                        _ => LOGGER.warn("Replacement MA request failed; retrying while playback intent is unchanged"),
                    }
                }
            }
        } else {
            tokio::select! {
                changed = intent.changed() => { if changed.is_err() { return; } }
                changed = connections.changed() => { if changed.is_err() { return; } }
                _ = ticks.tick() => {}
            }
        }
    }
}
