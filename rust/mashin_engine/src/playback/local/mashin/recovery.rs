//! Rejected audio loses its cache, not the user's playback intent or audible position.

use super::audio_adapter::MashinAudioAdapter;
use super::audio_adapter::OutputSnapshot;
use super::models::{OutputState, PlaybackIdentity};
use super::protocol::ControlCommand;
use super::receiver::{ConnectionState, ReceiverSnapshot};
use crate::logging::Logger;
use crate::music_assistant::client::{HttpCommandError, MusicAssistantClient, QueueCommand};
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

#[cfg(test)]
mod tests {
    use super::super::models::{test_stream, AudioProfileId};
    use super::*;
    use crate::audio::models::AudioCommand;
    use crate::music_assistant::client::tests::{server, server_with_request_handler};
    use serde_json::json;

    async fn rejected_output() -> MashinAudioAdapter {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let mut stream = test_stream();
        stream.media_start_position_millis = 162_000;
        output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(stream),
                prepared: None,
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        output.reject_retained_playback().await.unwrap();
        output
    }

    fn connected() -> (
        watch::Sender<ReceiverSnapshot>,
        watch::Receiver<ReceiverSnapshot>,
    ) {
        watch::channel(ReceiverSnapshot {
            control_connection: ConnectionState::Connected,
            audio_connection: ConnectionState::Connected,
            player_id: Some("player".into()),
            audio_profile: AudioProfileId::Flac,
        })
    }

    fn queues() -> serde_json::Value {
        json!([{"queue_id":"queue", "current_item":{"queue_id":"queue", "queue_item_id":"item", "name":"Retained"}}])
    }

    #[tokio::test]
    async fn failed_replacement_requests_retry_same_item_and_real_position() {
        let output = rejected_output().await;
        let failure = json!({"test_http_status":503});
        let (client, server) = server(vec![
            ("player_queues/all", queues()),
            ("player_queues/play_index", failure.clone()),
            ("player_queues/all", queues()),
            ("player_queues/play_index", failure),
            ("player_queues/all", queues()),
            ("player_queues/play_index", json!(null)),
        ]);
        let (_connection, connection) = connected();
        let worker = tokio::spawn(run(output.clone(), Arc::new(client), connection));
        let requests = tokio::time::timeout(
            Duration::from_secs(8),
            tokio::task::spawn_blocking(move || server.join().unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(requests.len(), 6);
        for request in requests
            .iter()
            .filter(|request| request["command"] == "player_queues/play_index")
        {
            assert_eq!(
                request["args"],
                json!({"queue_id":"queue", "index":"item", "seek_position":162})
            );
        }
        assert!(
            output.recovery().borrow().target.is_some(),
            "HTTP success is not an accepted stream"
        );
        worker.abort();
        let _ = worker.await;
    }

    #[tokio::test]
    async fn local_stop_during_queue_query_prevents_replacement_load() {
        let output = rejected_output().await;
        let local = output.clone();
        let (client, server) =
            server_with_request_handler(vec![("player_queues/all", queues())], move |_| {
                local.authorize_local_command(&AudioCommand::Stop { stream_id: None });
            });
        let (_connection, connection) = connected();
        let worker = tokio::spawn(run(output.clone(), Arc::new(client), connection));
        let requests = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || server.join().unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(requests.len(), 1);
        assert!(output.recovery().borrow().target.is_none());
        assert!(output.recovery().borrow().is_stopped);
        worker.abort();
        let _ = worker.await;
    }

    #[tokio::test]
    async fn changed_server_queue_item_cancels_replacement() {
        let output = rejected_output().await;
        let mut changed = queues();
        changed[0]["current_item"]["queue_item_id"] = json!("another-item");
        let (client, server) = server(vec![("player_queues/all", changed)]);
        let (_connection, connection) = connected();
        let worker = tokio::spawn(run(output.clone(), Arc::new(client), connection));
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || server.join().unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        let mut intent = output.recovery();
        tokio::time::timeout(Duration::from_secs(2), async {
            while intent.borrow_and_update().target.is_some() {
                intent.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        worker.abort();
        let _ = worker.await;
    }

    #[tokio::test]
    async fn rejection_keeps_pause_and_empty_wire_state_until_new_media() {
        let output = rejected_output().await;
        let wire = output.snapshot();
        assert_eq!(wire.resume.output_state, OutputState::Idle);
        assert!(wire.resume.received_streams.is_empty());
        assert!(wire.current.is_none());
        let original = output.recovery().borrow().target.clone().unwrap();
        assert_eq!(original.position_seconds, 162);
        assert!(original.is_paused);
        output.reject_retained_playback().await.unwrap();
        assert_eq!(output.recovery().borrow().target.as_ref(), Some(&original));
        output
            .execute(ControlCommand::SetMedia {
                command_id: 2,
                current: Some(test_stream()),
                prepared: None,
                media_revision: 2,
                initial_output_state: OutputState::Playing,
            })
            .await
            .unwrap();
        assert_eq!(output.snapshot().resume.output_state, OutputState::Paused);
        assert!(output.recovery().borrow().target.is_some());
        assert!(output.recovery().borrow().replacement_stream_id.is_some());
        let retry_revision = output.recovery().borrow().retry_revision;
        output.reject_retained_playback().await.unwrap();
        assert_eq!(output.recovery().borrow().retry_revision, retry_revision);
        assert!(output.recovery().borrow().replacement_stream_id.is_none());
    }

    #[tokio::test]
    async fn permission_denial_stops_recovery_without_authentication_loop() {
        for status in [401, 403] {
            let output = rejected_output().await;
            let (client, server) = server(vec![(
                "player_queues/all",
                json!({"test_http_status":status}),
            )]);
            let (_connection, connection) = connected();
            let worker = tokio::spawn(run(output.clone(), Arc::new(client), connection));
            tokio::time::timeout(
                Duration::from_secs(5),
                tokio::task::spawn_blocking(move || server.join().unwrap()),
            )
            .await
            .unwrap()
            .unwrap();
            let mut intent = output.recovery();
            tokio::time::timeout(Duration::from_secs(2), async {
                while !intent.borrow_and_update().is_stopped {
                    intent.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
            assert!(intent.borrow().target.is_none());
            worker.abort();
            let _ = worker.await;
        }
    }

    #[tokio::test]
    async fn local_pause_before_cleanup_preserves_immediate_intent() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(test_stream()),
                prepared: None,
                media_revision: 1,
                initial_output_state: OutputState::Playing,
            })
            .await
            .unwrap();
        assert!(!output.authorize_local_command(&AudioCommand::Pause { stream_id: None }));
        let mut snapshot = output.snapshot();
        snapshot
            .current
            .as_mut()
            .unwrap()
            .media_start_position_millis = 150_000;
        snapshot.resume.position_frame = 12 * 48_000;
        assert_eq!(
            RecoveryTarget::from_snapshot(&snapshot)
                .unwrap()
                .position_seconds,
            162
        );
        output.reject_retained_playback().await.unwrap();
        assert!(
            output
                .recovery()
                .borrow()
                .target
                .as_ref()
                .unwrap()
                .is_paused
        );
    }

    #[tokio::test]
    async fn pause_after_replacement_acceptance_still_reaches_native_output() {
        let output = rejected_output().await;
        output.authorize_local_command(&AudioCommand::Play { stream_id: None });
        output
            .execute(ControlCommand::SetMedia {
                command_id: 2,
                current: Some(test_stream()),
                prepared: None,
                media_revision: 2,
                initial_output_state: OutputState::Playing,
            })
            .await
            .unwrap();
        let pause = AudioCommand::Pause { stream_id: None };
        assert!(!output.authorize_local_command(&pause));
        output.pipeline().execute(pause).await.unwrap();
        assert_eq!(output.snapshot().resume.output_state, OutputState::Paused);
        assert!(
            output
                .recovery()
                .borrow()
                .target
                .as_ref()
                .unwrap()
                .is_paused
        );
    }

    #[tokio::test]
    async fn play_pause_volume_and_stop_apply_to_pending_intent() {
        let output = rejected_output().await;
        assert!(output.authorize_local_command(&AudioCommand::Play { stream_id: None }));
        assert!(
            !output
                .recovery()
                .borrow()
                .target
                .as_ref()
                .unwrap()
                .is_paused
        );
        assert!(!output.authorize_local_command(&AudioCommand::SetVolume { volume_percent: 40 }));
        assert!(output.recovery().borrow().target.is_some());
        assert!(output.authorize_local_command(&AudioCommand::Pause { stream_id: None }));
        assert!(
            output
                .recovery()
                .borrow()
                .target
                .as_ref()
                .unwrap()
                .is_paused
        );
        assert!(!output.authorize_local_command(&AudioCommand::Stop { stream_id: None }));
        output.reject_retained_playback().await.unwrap();
        assert!(output.recovery().borrow().target.is_none());
    }

    #[test]
    fn retries_do_not_expire_and_backoff_is_capped() {
        let mut now = Instant::now();
        let mut backoff = RecoveryBackoff::new(1, now);
        for attempt in 0..10_000 {
            assert!(backoff.is_due(1, now));
            backoff.attempted(now);
            let delay = Duration::from_secs((1_u64 << attempt.min(5)).min(30));
            assert!(!backoff.is_due(1, now + delay - Duration::from_nanos(1)));
            now += delay;
        }
        assert!(backoff.is_due(2, now));
    }

    #[test]
    fn changed_intent_invalidates_captured_requests() {
        let mut state = RecoveryState {
            target: Some(RecoveryTarget {
                playback: super::super::models::test_stream().playback,
                position_seconds: 162,
                is_paused: false,
            }),
            ..Default::default()
        };
        let original = state.clone();
        state.set_paused(true);
        assert_ne!(state.revision, original.revision);
        assert!(state.target.as_ref().unwrap().is_paused);
        assert_eq!(state.target.as_ref().unwrap().position_seconds, 162);
        state.cancel();
        assert!(state.target.is_none());
        assert_ne!(state.revision, original.revision);
    }
}

// The session owns this worker; no API request may outlive its captured local intent.
pub(super) async fn run(
    output: MashinAudioAdapter,
    client: Arc<MusicAssistantClient>,
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
                        Ok(Err(HttpCommandError::Http { status: 401 | 403, .. })) => {
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
