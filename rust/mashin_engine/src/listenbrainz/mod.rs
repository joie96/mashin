//! Core-owned local scrobbling, independent of Flutter and Music Assistant recommendations.
//! Playback observation, durable writes and HTTP submission run outside the audio thread.

mod client;
mod outbox;
mod tracker;

use crate::core::core::CORE_COORDINATOR;
use crate::logging::Logger;
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::PlayState;
use crate::playback::models::PlaybackEngineKind;
pub(crate) use client::{validate_credentials, ListenBrainzCredentials};
use client::{Listen, ListenBrainzClient, SubmissionOutcome};
use outbox::{ListenOutbox, PendingListen};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tracker::ListenTracker;

static START: Once = Once::new();
static PLAYBACK_GENERATION: AtomicU64 = AtomicU64::new(0);
static LOGGER: Logger = Logger::new(module_path!());

// Worker lifecycle and playback identity.
// Android service ownership keeps these workers alive without a Flutter client.
pub(crate) fn start(runtime: &Runtime) {
    if cfg!(test) {
        return;
    }
    START.call_once(|| {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        runtime.spawn(observe_playback(sender));
        runtime.spawn(submit_pending_listens(receiver));
    });
}

// New local playback, including repeat, gets a distinct identity; seek does not.
pub(crate) fn record_playback_start() {
    PLAYBACK_GENERATION.fetch_add(1, Ordering::Relaxed);
}

// Unchanged credentials allow settings changes offline without another token-validation request.
pub(crate) fn validate_settings_update(mut update: crate::core::models::SessionSettingsUpdate)
    -> Result<crate::core::models::SessionSettingsUpdate, String> {
    let stored = CORE_COORDINATOR.lock().map_err(|_| "core lock poisoned".to_string())?.listenbrainz_credentials();
    if stored != update.listenbrainz {
        update.listenbrainz = validate_credentials(update.listenbrainz)?;
    }
    Ok(update)
}

struct CompletedSubmission {
    username: String,
    count: usize,
    outcome: SubmissionOutcome,
}

// Playback observations and worker coordination.
async fn observe_playback(sender: tokio::sync::mpsc::UnboundedSender<PendingListen>) {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut tracker = ListenTracker::default();
    let mut account = ListenBrainzCredentials::default();
    let mut previous_tick = Instant::now();
    loop {
        interval.tick().await;
        let now = Instant::now();
        let elapsed = now.duration_since(previous_tick).as_millis() as u64;
        previous_tick = now;
        let (credentials, player, track, generation) = {
            let Ok(core) = CORE_COORDINATOR.try_lock() else {
                tracker.suspend();
                continue;
            };
            let credentials = core.listenbrainz_credentials();
            let player = core.player_state();
            let track = core
                .queue()
                .items
                .into_iter()
                .find(|item| Some(&item.queue_item_id) == player.current_queue_item_id.as_ref())
                .and_then(|item| match item.media_item {
                    Some(MediaItem::Track(track)) => Some(track),
                    _ => None,
                });
            (
                credentials,
                player,
                track,
                PLAYBACK_GENERATION.load(Ordering::Relaxed),
            )
        };
        if credentials != account {
            tracker = ListenTracker::default();
            account = credentials;
        }
        if account.token.is_empty()
            || player.active_engine_kind == PlaybackEngineKind::Remote
            || matches!(
                player.playback_state,
                PlayState::Idle | PlayState::Uninitialized | PlayState::Error
            )
        {
            tracker = ListenTracker::default();
            continue;
        }
        let Some(track) = track else {
            tracker = ListenTracker::default();
            continue;
        };
        let Some(position) = player.position_millis else {
            tracker.suspend();
            continue;
        };
        let queue_item_id = player
            .current_queue_item_id
            .as_ref()
            .map(|id| id.0.as_str())
            .unwrap_or("");
        let playback_id = format!("{generation}:{queue_item_id}");
        if tracker.observe(
            &playback_id,
            position,
            player
                .duration_millis
                .unwrap_or(i64::from(track.duration) * 1_000),
            player.playback_state == PlayState::Playing,
            elapsed,
            chrono::Utc::now().timestamp(),
        ) {
            if let Some(listen) = Listen::from_track(&track, tracker.started_at) {
                if sender
                    .send(PendingListen {
                        username: account.username.clone(),
                        listen,
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

// HTTP runs in a separate task so pending listens can be persisted during an in-flight request.
async fn submit_pending_listens(mut receiver: tokio::sync::mpsc::UnboundedReceiver<PendingListen>) {
    let mut outbox = match ListenOutbox::load_default() {
        Ok(outbox) => outbox,
        Err(error) => {
            LOGGER.error(error);
            return;
        }
    };
    let client = match ListenBrainzClient::new() {
        Ok(client) => client,
        Err(error) => {
            LOGGER.error(error);
            return;
        }
    };
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (completed_sender, mut completed_receiver) =
        tokio::sync::mpsc::unbounded_channel::<CompletedSubmission>();
    let mut is_submitting = false;
    let mut retry_at = Instant::now();
    loop {
        tokio::select! {
            item = receiver.recv() => {
                let Some(item) = item else { return; };
                outbox.enqueue(item);
            }
            completed = completed_receiver.recv() => {
                let Some(completed) = completed else { return; };
                is_submitting = false;
                match completed.outcome {
                    SubmissionOutcome::Accepted => {
                        outbox.acknowledge(&completed.username, completed.count);
                        LOGGER.info(format!("Submitted {} ListenBrainz listens", completed.count));
                    }
                    SubmissionOutcome::Retry { status, after } => {
                        retry_at = Instant::now() + after;
                        LOGGER.warn(format!("ListenBrainz submission deferred (HTTP {status:?}); listens remain pending"));
                    }
                }
            }
            _ = interval.tick() => {}
        }
        if let Err(error) = outbox.persist() {
            LOGGER.error(error);
            continue;
        }
        if is_submitting || Instant::now() < retry_at {
            continue;
        }
        let credentials = {
            let Ok(core) = CORE_COORDINATOR.try_lock() else {
                continue;
            };
            core.listenbrainz_credentials()
        };
        if credentials.token.is_empty() {
            continue;
        }
        let batch = outbox.batch(&credentials.username);
        if batch.is_empty() {
            continue;
        }
        is_submitting = true;
        let client = client.clone();
        let completed_sender = completed_sender.clone();
        tokio::spawn(async move {
            let outcome = client.submit(&credentials, &batch).await;
            let _ = completed_sender.send(CompletedSubmission {
                username: credentials.username,
                count: batch.len(),
                outcome,
            });
        });
    }
}
