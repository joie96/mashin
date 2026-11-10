//! Application playback commands preserve backend ownership and publish only changed projections.

use crate::application::events::ApplicationEvent;
use crate::application::{errors::ApplicationError, Application};
use crate::domain::media::MediaItem;
use crate::playback::engine::PlaybackEngine;
use crate::playback::models::QueueOption;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};

pub(crate) fn collect_media_session_events(
    state: MediaSessionState,
    queue_before: Queue,
    player_state_before: PlayerState,
) -> Vec<ApplicationEvent> {
    let mut events = Vec::with_capacity(2);
    if !state.queue.has_same_structure_as(&queue_before) {
        events.push(ApplicationEvent::QueueChanged { queue: state.queue });
    }
    if state.player_state != player_state_before {
        events.push(ApplicationEvent::PlayerStateChanged {
            player_state: state.player_state,
        });
    }
    events
}

impl Application {
    // Playback commands share application serialization and events without duplicating the engine API.
    pub(crate) fn execute_playback_command_and_collect_events<F>(
        &self,
        operation: F,
    ) -> Result<Vec<ApplicationEvent>, ApplicationError>
    where
        F: FnOnce(&mut PlaybackEngine) -> Result<(), String>,
    {
        self.execute_command_and_collect_events(|coordinator| operation(&mut coordinator.playback))
    }

    // The backend receives complete media payloads and owns queue resolution.
    pub(crate) fn execute_media_command<F>(
        &self,
        media_items: Vec<MediaItem>,
        option: QueueOption,
        operation: F,
    ) -> Result<Vec<ApplicationEvent>, ApplicationError>
    where
        F: FnOnce(&mut PlaybackEngine, Vec<MediaItem>) -> Result<(), String>,
    {
        let _command_guard = self
            .command_serializer
            .lock()
            .map_err(|_| ApplicationError::internal("core command serializer lock poisoned"))?;
        let is_starting =
            !media_items.is_empty() && matches!(option, QueueOption::Play | QueueOption::Replace);
        let start_request = if is_starting {
            Some(
                self.coordinator
                    .lock()
                    .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?
                    .playback
                    .begin_start(crate::playback::start::StartKind::Media)
                    .map_err(ApplicationError::command_failed)?,
            )
        } else {
            None
        };
        drop(_command_guard);
        let result = self.runtime.block_on(self.execute_resolved_media_command(
            media_items,
            start_request.as_ref(),
            None,
            None,
            operation,
        ));
        if result.is_err() {
            if let Some(request) = start_request {
                request.complete(false);
            }
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            let event = ApplicationEvent::PlayerStateChanged {
                player_state: coordinator.playback.player_state(),
            };
            let event_publisher = coordinator.session_events();
            drop(coordinator);
            event_publisher.publish(event);
        }
        result
    }

    // Radio I/O leaves transport responsive; each queue write validates its original selection.
    pub(crate) fn start_radio_and_execute_command(
        &self,
        media_items: Vec<MediaItem>,
    ) -> Result<Vec<ApplicationEvent>, ApplicationError> {
        let (generation, selection_revision, event_publisher) = {
            let _command_guard = self
                .command_serializer
                .lock()
                .map_err(|_| ApplicationError::internal("core command serializer lock poisoned"))?;
            let mut coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            coordinator.connection.as_ref().ok_or_else(|| {
                ApplicationError::command_failed("a restored session is required to start radio")
            })?;
            (
                coordinator.session_generation,
                coordinator.playback.reserve_media_selection(),
                coordinator.session_events(),
            )
        };
        let providers = self.provider_repository(generation)?;
        let instances = self
            .runtime
            .block_on(async { providers.read_instances().await?.resolve().await });
        let catalog = self.catalog_repository(generation)?;
        self.runtime
            .block_on(crate::application::radio::start_radio(
                catalog.as_ref(),
                instances
                    .as_ref()
                    .ok()
                    .and_then(|update| update.items.as_deref()),
                media_items,
                |media, option| {
                    let event_publisher = event_publisher.clone();
                    async move {
                        let expected_revision = if option == QueueOption::Replace {
                            selection_revision
                        } else {
                            selection_revision.wrapping_add(1)
                        };
                        let events = self
                            .execute_resolved_media_command(
                                media,
                                None,
                                Some(expected_revision),
                                Some(generation),
                                |playback, media| {
                                    playback.queue_play_media(media, option, None, false)
                                },
                            )
                            .await
                            .map_err(|error| error.message)?;
                        for event in events {
                            event_publisher.publish(event);
                        }
                        Ok(())
                    }
                },
            ))
            .map_err(ApplicationError::command_failed)?;
        Ok(Vec::new())
    }

    // Shared media execution releases core ownership while the queued backend commands await I/O.
    async fn execute_resolved_media_command<F>(
        &self,
        media_items: Vec<MediaItem>,
        start_request: Option<&crate::playback::start::PlaybackStartRequest>,
        selection_revision: Option<u64>,
        expected_generation: Option<u64>,
        operation: F,
    ) -> Result<Vec<ApplicationEvent>, ApplicationError>
    where
        F: FnOnce(&mut PlaybackEngine, Vec<MediaItem>) -> Result<(), String>,
    {
        let command_guard = self
            .command_serializer
            .lock()
            .map_err(|_| ApplicationError::internal("core command serializer lock poisoned"))?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        if start_request.is_some_and(|request| !request.is_pending()) {
            return Err(ApplicationError::command_failed(
                "playback start was cancelled or replaced",
            ));
        }
        if expected_generation
            .is_some_and(|generation| coordinator.session_generation != generation)
        {
            return Err(ApplicationError::command_failed(
                "session changed while resolving radio",
            ));
        }
        if selection_revision
            .is_some_and(|revision| coordinator.playback.media_selection_revision() != revision)
        {
            return Err(ApplicationError::command_failed(
                "radio selection changed while resolving media",
            ));
        }
        let queue_before = coordinator.playback.queue();
        let player_state_before = coordinator.playback.player_state();
        let result = operation(&mut coordinator.playback, media_items);
        let commands = coordinator
            .take_pending_commands()
            .map_err(ApplicationError::command_failed)?;
        result.map_err(ApplicationError::command_failed)?;
        drop(coordinator);
        drop(command_guard);
        for command in commands {
            command.await.map_err(ApplicationError::command_failed)?;
        }
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        Ok(collect_media_session_events(
            coordinator.media_session_state(),
            queue_before,
            player_state_before,
        ))
    }
}
