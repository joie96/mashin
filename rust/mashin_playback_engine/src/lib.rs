#![allow(linker_messages)]

//! Central FFI facade and playback coordinator.
//!
//! The coordinator owns queue and playback state. Engine-specific behavior is
//! delegated to the active local engine; Sendspin is the first implementation.

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod audio;
mod cache;
mod logging;
mod offline;
mod sendspin;

use sendspin::engine::SendspinEngine;

static COORDINATOR: Lazy<Mutex<PlaybackCoordinator>> =
    Lazy::new(|| Mutex::new(PlaybackCoordinator::new()));
static LAST_ERROR: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
type EventCallback = extern "C" fn(*mut c_char);
static EVENT_CALLBACK: Lazy<Mutex<Option<EventCallback>>> = Lazy::new(|| Mutex::new(None));
static PLAYBACK_RUNTIME: Lazy<Mutex<Option<(Arc<AtomicBool>, JoinHandle<()>)>>> =
    Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "android")]
static ANDROID_APP_CONTEXT: Lazy<Mutex<Option<jni::objects::GlobalRef>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackEngineKind {
    Sendspin,
    Offline,
}

impl PlaybackEngineKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sendspin => "sendspin",
            Self::Offline => "offline",
        }
    }
}

struct PlaybackCoordinator {
    active_engine: PlaybackEngineKind,
    sendspin: SendspinEngine,
    offline_available: bool,
    emission: EmissionState,
}

struct EmissionState {
    queue_revision: Option<u64>,
    cache_items: HashMap<String, String>,
    last_playback_state: Option<String>,
    last_playback_at: Option<Instant>,
}

impl PlaybackCoordinator {
    fn new() -> Self {
        Self {
            active_engine: PlaybackEngineKind::Sendspin,
            sendspin: SendspinEngine::new(),
            offline_available: false,
            emission: EmissionState {
                queue_revision: None,
                cache_items: HashMap::new(),
                last_playback_state: None,
                last_playback_at: None,
            },
        }
    }

    fn select_active_engine(&mut self, engine: &str) -> Result<(), String> {
        match engine.trim().to_ascii_lowercase().as_str() {
            "sendspin" | "local_sendspin" => {
                self.active_engine = PlaybackEngineKind::Sendspin;
                Ok(())
            }
            "offline" | "local_offline" if self.offline_available => {
                self.active_engine = PlaybackEngineKind::Offline;
                Ok(())
            }
            "offline" | "local_offline" => {
                Err("offline playback engine is not available yet".to_string())
            }
            value => Err(format!("unsupported playback engine: {value}")),
        }
    }

    fn configure_and_select_engine(
        &mut self,
        engine: &str,
        config_json: Option<&str>,
    ) -> Result<(), String> {
        let normalized_engine = engine.trim().to_ascii_lowercase();
        match normalized_engine.as_str() {
            "sendspin" | "local_sendspin" => {
                let config_json =
                    config_json.ok_or_else(|| "Sendspin engine config is required".to_string())?;
                let config: Value = serde_json::from_str(config_json)
                    .map_err(|error| format!("invalid Sendspin engine config: {error}"))?;
                self.sendspin.configure(config)?;
                self.active_engine = PlaybackEngineKind::Sendspin;
                Ok(())
            }
            "offline" | "local_offline" => self.select_active_engine(&normalized_engine),
            value => Err(format!("unsupported playback engine: {value}")),
        }
    }

    fn initialize_sendspin(&mut self, config_json: &str) -> Result<(), String> {
        let config: Value = serde_json::from_str(config_json)
            .map_err(|error| format!("invalid playback engine config: {error}"))?;
        self.sendspin.init(config);
        Ok(())
    }

    fn activate_selected_engine(&mut self) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.connect(),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn deactivate_active_engine(&mut self) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.disconnect();
        }
    }

    fn pump_active_engine(&mut self) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.pump_playback();
        }
    }

    fn play_media(&mut self, media_items_json: &str) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.play_media(media_items_json),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn play_media_next(&mut self, items: &str, replace_following: bool) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.play_media_next(items, replace_following),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn play_media_last(&mut self, items: &str) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.play_media_last(items),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn clear_queue(&mut self) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.clear_queue(),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn move_queue_item(&mut self, item_id: &str, shift: i32) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.move_queue_item(item_id, shift),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn delete_queue_item(&mut self, item_id: &str) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.delete_queue_item(item_id),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.play_queue_index(index),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn transport_play(&mut self) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.transport_play(),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn transport_pause(&mut self) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.transport_pause();
        }
    }

    fn transport_stop(&mut self) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.transport_stop();
        }
    }

    fn transport_next(&mut self) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.transport_next(),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn transport_previous(&mut self) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.transport_previous(),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.transport_seek(seconds),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn set_volume(&mut self, volume: u8) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.set_volume(volume);
        }
    }

    fn set_muted(&mut self, muted: bool) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.set_muted(muted);
        }
    }

    fn set_shuffle(&mut self, enabled: bool) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.set_shuffle(enabled);
        }
    }

    fn set_repeat_mode(&mut self, mode: &str) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.set_repeat_mode(mode);
        }
    }

    fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.set_preferred_codec(codec),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn collect_event_messages(&mut self, force_playback: bool) -> Result<Vec<String>, String> {
        let snapshot = self.sendspin.snapshot();
        let mut messages = Vec::new();

        let queue_changed = self.emission.queue_revision != Some(self.sendspin.queue_revision());
        if queue_changed {
            messages.push(
                serde_json::to_string(&json!({
                    "type": "queue",
                    "queue": snapshot.queue,
                }))
                .map_err(|error| format!("serialize queue event failed: {error}"))?,
            );
            self.emission.queue_revision = Some(self.sendspin.queue_revision());
        }

        let cache_updates = self.sendspin.cache_updates();
        let mut current_cache_items = HashMap::with_capacity(cache_updates.len());
        let mut changed_cache_updates = Vec::new();
        for update in cache_updates {
            let update_json = serde_json::to_string(&update)
                .map_err(|error| format!("serialize cache update failed: {error}"))?;
            let queue_item_id = update.queue_item_id.clone();
            if self.emission.cache_items.get(&queue_item_id) != Some(&update_json) {
                changed_cache_updates.push(update);
            }
            current_cache_items.insert(queue_item_id, update_json);
        }
        self.emission.cache_items = current_cache_items;
        if !queue_changed && !changed_cache_updates.is_empty() {
            messages.push(
                serde_json::to_string(&json!({
                    "type": "cache",
                    "items": changed_cache_updates,
                }))
                .map_err(|error| format!("serialize cache event failed: {error}"))?,
            );
        }

        let now = Instant::now();
        let playback_state_changed =
            self.emission.last_playback_state.as_deref() != Some(snapshot.playback_state.as_str());
        if force_playback
            || playback_state_changed
            || self
                .emission
                .last_playback_at
                .map(|last| now.duration_since(last) >= Duration::from_millis(250))
                .unwrap_or(true)
        {
            messages.push(
                serde_json::to_string(&json!({
                    "type": "playback",
                    "engine": self.active_engine.as_str(),
                    "offlineAvailable": self.offline_available,
                    "connectionState": snapshot.connection_state,
                    "playbackState": snapshot.playback_state,
                    "localPlaybackPositionMillis": snapshot.local_playback_position_millis,
                    "volume": snapshot.volume,
                    "muted": snapshot.muted,
                    "shuffleEnabled": snapshot.shuffle_enabled,
                    "repeatMode": snapshot.repeat_mode,
                    "currentIndex": snapshot.current_index,
                }))
                .map_err(|error| format!("serialize playback event failed: {error}"))?,
            );
            self.emission.last_playback_state = Some(snapshot.playback_state.clone());
            self.emission.last_playback_at = Some(now);
        }
        Ok(messages)
    }
}

fn record_and_emit_error(message: impl Into<String>) {
    let message = message.into();
    if let Ok(mut last_error) = LAST_ERROR.lock() {
        *last_error = message.clone();
    }
    emit_error_event(&message);
}

fn emit_error_event(message: &str) {
    emit_json_value(json!({
        "playbackState": "error",
        "error": message,
    }));
}

fn start_playback_runtime() {
    let Ok(mut runtime) = PLAYBACK_RUNTIME.lock() else {
        return;
    };
    if runtime.is_some() {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop);
    let task = std::thread::spawn(move || {
        while !stop_for_thread.load(Ordering::Relaxed) {
            let messages = match COORDINATOR.lock() {
                Ok(mut coordinator) => {
                    coordinator.pump_active_engine();
                    coordinator.collect_event_messages(false).ok()
                }
                Err(_) => None,
            };
            if let Some(messages) = messages {
                for message in messages {
                    emit_json_text(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    });
    *runtime = Some((stop, task));
}

fn stop_playback_runtime() {
    let Ok(mut runtime) = PLAYBACK_RUNTIME.lock() else {
        return;
    };
    if let Some((stop, task)) = runtime.take() {
        stop.store(true, Ordering::Relaxed);
        let _ = task.join();
    }
}

fn emit_json_value(value: Value) {
    if let Ok(text) = serde_json::to_string(&value) {
        emit_json_text(&text);
    }
}

fn emit_json_text(text: &str) {
    let callback = EVENT_CALLBACK.lock().ok().and_then(|callback| *callback);
    let Some(callback) = callback else {
        return;
    };
    let Ok(pointer) = CString::new(text) else {
        return;
    };
    callback(pointer.into_raw());
}

fn read_required_utf8(pointer: *const c_char, name: &str) -> Result<String, String> {
    if pointer.is_null() {
        return Err(format!("{name} is null"));
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map(str::to_owned)
        .map_err(|error| format!("{name} is not valid UTF-8: {error}"))
}

fn read_optional_utf8(pointer: *const c_char, name: &str) -> Result<Option<String>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    read_required_utf8(pointer, name).map(Some)
}

fn run_coordinator_command<F>(operation: F) -> i32
where
    F: FnOnce(&mut PlaybackCoordinator) -> Result<(), String>,
{
    let result = match COORDINATOR.lock() {
        Ok(mut coordinator) => {
            operation(&mut coordinator).and_then(|()| coordinator.collect_event_messages(true))
        }
        Err(_) => Err("playback coordinator lock poisoned".to_string()),
    };
    match result {
        Ok(messages) => {
            for message in messages {
                emit_json_text(&message);
            }
            0
        }
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_example_mashin_1flutter_MainActivity_nativeSetAndroidContext(
    env: jni::JNIEnv,
    _class: jni::objects::JObject,
    context: jni::objects::JObject,
) {
    if let Ok(global_ref) = env.new_global_ref(context) {
        if let Ok(mut stored) = ANDROID_APP_CONTEXT.lock() {
            *stored = Some(global_ref);
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_register_event_callback(
    callback: Option<EventCallback>,
) -> i32 {
    match EVENT_CALLBACK.lock() {
        Ok(mut stored) => {
            *stored = callback;
            0
        }
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_log_level(level: *const c_char) -> i32 {
    match read_required_utf8(level, "log_level").and_then(|level| logging::set_minimum_level(&level)) {
        Ok(()) => 0,
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_active_engine(
    engine: *const c_char,
    config_json: *const c_char,
) -> i32 {
    let engine = match read_required_utf8(engine, "engine") {
        Ok(value) => value,
        Err(error) => {
            record_and_emit_error(error);
            return -1;
        }
    };
    let config_json = match read_optional_utf8(config_json, "engine_config_json") {
        Ok(value) => value,
        Err(error) => {
            record_and_emit_error(error);
            return -1;
        }
    };
    run_coordinator_command(|coordinator| {
        coordinator.configure_and_select_engine(&engine, config_json.as_deref())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_initialize(config_json: *const c_char) -> i32 {
    match read_required_utf8(config_json, "config_json") {
        Ok(config) => {
            run_coordinator_command(|coordinator| coordinator.initialize_sendspin(&config))
        }
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_activate() -> i32 {
    let result = run_coordinator_command(|coordinator| coordinator.activate_selected_engine());
    if result == 0 {
        start_playback_runtime();
    }
    result
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_deactivate() -> i32 {
    stop_playback_runtime();
    run_coordinator_command(|coordinator| {
        coordinator.deactivate_active_engine();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media(media_items_json: *const c_char) -> i32 {
    match read_required_utf8(media_items_json, "media_items_json") {
        Ok(media_items) => {
            run_coordinator_command(|coordinator| coordinator.play_media(&media_items))
        }
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

fn play_media_next_command(items_json: *const c_char, replace_following: bool) -> i32 {
    match read_required_utf8(items_json, "media_items_json") {
        Ok(items) => run_coordinator_command(|coordinator| {
            coordinator.play_media_next(&items, replace_following)
        }),
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media_next(items: *const c_char) -> i32 {
    play_media_next_command(items, false)
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media_replace_next(items: *const c_char) -> i32 {
    play_media_next_command(items, true)
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media_last(items: *const c_char) -> i32 {
    match read_required_utf8(items, "media_items_json") {
        Ok(items) => run_coordinator_command(|coordinator| coordinator.play_media_last(&items)),
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_preferred_codec(codec: *const c_char) -> i32 {
    match read_required_utf8(codec, "codec") {
        Ok(codec) => run_coordinator_command(|coordinator| coordinator.set_preferred_codec(&codec)),
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_clear_queue() -> i32 {
    run_coordinator_command(|coordinator| coordinator.clear_queue())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_move_queue_item(
    item_id: *const c_char,
    shift: i32,
) -> i32 {
    match read_required_utf8(item_id, "queue_item_id") {
        Ok(item_id) => {
            run_coordinator_command(|coordinator| coordinator.move_queue_item(&item_id, shift))
        }
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_delete_queue_item(item_id: *const c_char) -> i32 {
    match read_required_utf8(item_id, "queue_item_id") {
        Ok(item_id) => {
            run_coordinator_command(|coordinator| coordinator.delete_queue_item(&item_id))
        }
        Err(error) => {
            record_and_emit_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_queue_index(index: i32) -> i32 {
    if index < 0 {
        return -1;
    }
    run_coordinator_command(|coordinator| coordinator.play_queue_index(index as usize))
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_play() -> i32 {
    run_coordinator_command(|coordinator| coordinator.transport_play())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_pause() -> i32 {
    run_coordinator_command(|coordinator| {
        coordinator.transport_pause();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_stop() -> i32 {
    run_coordinator_command(|coordinator| {
        coordinator.transport_stop();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_next() -> i32 {
    run_coordinator_command(|coordinator| coordinator.transport_next())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_previous() -> i32 {
    run_coordinator_command(|coordinator| coordinator.transport_previous())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_seek(seconds: f64) -> i32 {
    run_coordinator_command(|coordinator| coordinator.transport_seek(seconds))
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_shuffle(enabled: i32) -> i32 {
    run_coordinator_command(|coordinator| {
        coordinator.set_shuffle(enabled != 0);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_repeat_mode(mode: i32) -> i32 {
    let mode = match mode {
        1 => "one",
        2 => "all",
        _ => "off",
    };
    run_coordinator_command(|coordinator| {
        coordinator.set_repeat_mode(mode);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_volume(volume: i32) -> i32 {
    run_coordinator_command(|coordinator| {
        coordinator.set_volume(volume.clamp(0, 100) as u8);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_muted(muted: i32) -> i32 {
    run_coordinator_command(|coordinator| {
        coordinator.set_muted(muted != 0);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_get_last_error() -> *mut c_char {
    let error = LAST_ERROR
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| "last error lock poisoned".to_string());
    CString::new(error)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_free_string(pointer: *mut c_char) {
    if pointer.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(pointer));
    }
}
