//! Engine selection and canonical media-session state projection.

use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::{MediaItem, Track};
use crate::music_assistant::models::queue::RepeatMode;
use crate::playback::engine::PlaybackEngine;
use crate::playback::models::{
    PlaybackConnectionState, PlaybackEngineConfiguration, PlaybackEngineKind, PlayerState, Queue,
};
use crate::sendspin::engine::SendspinEngine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) struct PlaybackCoordinator {
    active_engine: PlaybackEngineKind,
    engines: HashMap<PlaybackEngineKind, Arc<Mutex<Box<dyn PlaybackEngine>>>>,
}

impl PlaybackCoordinator {
    // Construction and engine configuration.
    pub(crate) fn new() -> Self {
        Self {
            active_engine: PlaybackEngineKind::Sendspin,
            engines: HashMap::from([(
                PlaybackEngineKind::Sendspin,
                Arc::new(Mutex::new(
                    Box::new(SendspinEngine::new()) as Box<dyn PlaybackEngine>
                )),
            )]),
        }
    }

    // Engine configuration and selection.
    pub(crate) fn configure_and_select_engine(
        &mut self,
        configuration: PlaybackEngineConfiguration,
    ) -> Result<(), String> {
        let requested_engine = configuration.kind();
        if !self.engines.contains_key(&requested_engine) {
            return Err(format!(
                "playback engine '{}' is not implemented",
                requested_engine.display_name()
            ));
        }

        if requested_engine == self.active_engine {
            if self.player_state().connection_state != PlaybackConnectionState::Offline {
                self.deactivate_active_engine();
            }
            return self.with_active_engine_mut(|engine| engine.configure(configuration));
        }

        self.engines
            .get_mut(&requested_engine)
            .expect("requested playback engine must be registered")
            .lock()
            .map_err(|_| "playback engine lock poisoned".to_string())?
            .configure(configuration)?;
        self.deactivate_active_engine();
        self.active_engine = requested_engine;
        Ok(())
    }

    // Playback lifecycle.
    pub(crate) fn activate_selected_engine(&mut self) -> Result<(), String> {
        let engine_handle = self.active_engine_handle();
        let mut engine = engine_handle
            .lock()
            .map_err(|_| "playback engine lock poisoned".to_string())?;
        engine.activate()?;
        engine.start_background_work(Arc::clone(&engine_handle));
        Ok(())
    }

    pub(crate) fn deactivate_active_engine(&mut self) {
        let engine_handle = self.active_engine_handle();
        if let Ok(mut engine) = engine_handle.lock() {
            engine.stop_background_work();
            engine.deactivate();
        };
    }

    pub(crate) fn active_engine_kind(&self) -> PlaybackEngineKind {
        self.active_engine
    }

    // Shutdown clears every registered engine so future output modes cannot retain a session.
    pub(crate) fn shutdown(&mut self) {
        for engine_handle in self.engines.values() {
            if let Ok(mut engine) = engine_handle.lock() {
                engine.stop_background_work();
                engine.shutdown();
            }
        }
    }

    // Queue commands.
    pub(crate) fn play_media(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.queue_replace(media_items))
    }

    pub(crate) fn play_media_next(
        &mut self,
        media_items: Vec<MediaItem>,
        replace_following: bool,
    ) -> Result<(), String> {
        self.with_active_engine_mut(|engine| {
            engine.queue_insert_next(media_items, replace_following)
        })
    }

    pub(crate) fn play_media_last(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.queue_append(media_items))
    }

    pub(crate) fn append_radio_tracks(
        &mut self,
        seed_tracks: Vec<Track>,
        radio_tracks: Vec<Track>,
    ) -> Result<(), String> {
        self.with_active_engine_mut(|engine| {
            engine.queue_append_radio_tracks(seed_tracks, radio_tracks)
        })
    }

    pub(crate) fn clear_queue(&mut self) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.queue_clear())
    }

    pub(crate) fn move_queue_item(
        &mut self,
        item_id: QueueItemId,
        shift: i32,
    ) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.queue_move(item_id, shift))
    }

    pub(crate) fn delete_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.queue_remove(item_id))
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.queue_play(index))
    }

    // Playback transport commands.
    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.transport_play())
    }

    pub(crate) fn transport_pause(&mut self) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.transport_pause();
            Ok(())
        });
    }

    pub(crate) fn transport_stop(&mut self) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.transport_stop();
            Ok(())
        });
    }

    pub(crate) fn transport_next(&mut self) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.transport_next())
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.transport_previous())
    }

    pub(crate) fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.with_active_engine_mut(|engine| engine.transport_seek(seconds))
    }

    // Playback preference commands.
    pub(crate) fn set_volume(&mut self, volume: u8) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.set_volume(volume);
            Ok(())
        });
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.set_muted(muted);
            Ok(())
        });
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.set_shuffle(enabled);
            Ok(())
        });
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.set_repeat_mode(mode);
            Ok(())
        });
    }

    pub(crate) fn set_autoplay(&mut self, is_enabled: bool) {
        let _ = self.with_active_engine_mut(|engine| {
            engine.set_autoplay(is_enabled);
            Ok(())
        });
    }

    pub(crate) fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        let active_engine = self.active_engine;
        self.with_active_engine_mut(|engine| {
            engine
                .preferred_codec_configuration()
                .ok_or_else(|| {
                    format!(
                        "playback engine '{}' does not support preferred codec configuration",
                        active_engine.display_name()
                    )
                })?
                .set_preferred_codec(codec)
        })
    }

    // Playback state queries.
    pub(crate) fn queue(&self) -> Queue {
        self.active_engine_handle()
            .lock()
            .map(|engine| engine.queue())
            .unwrap_or_default()
    }

    pub(crate) fn player_state(&self) -> PlayerState {
        let mut player_state = self
            .active_engine_handle()
            .lock()
            .map(|engine| engine.player_state())
            .unwrap_or_default();
        player_state.active_engine_kind = self.active_engine;
        player_state
    }

    // Active engine access.
    fn active_engine_handle(&self) -> Arc<Mutex<Box<dyn PlaybackEngine>>> {
        self.engines
            .get(&self.active_engine)
            .expect("active playback engine must be registered")
            .clone()
    }

    fn with_active_engine_mut<T>(
        &self,
        operation: impl FnOnce(&mut dyn PlaybackEngine) -> Result<T, String>,
    ) -> Result<T, String> {
        let engine_handle = self.active_engine_handle();
        let mut engine = engine_handle
            .lock()
            .map_err(|_| "playback engine lock poisoned".to_string())?;
        operation(engine.as_mut())
    }
}
