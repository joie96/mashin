//! Central FFI facade and playback coordinator.
//!
//! The coordinator owns queue and playback state. Engine-specific behavior is
//! delegated to the active local engine; Sendspin is the first implementation.

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

mod audio;
mod cache;
mod offline;
mod sendspin;

use sendspin::engine::{SendspinEngine, SendspinSnapshot};

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

fn log_error(message: &str) {
    eprintln!("[mashin_playback_engine][E] {message}");
}

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
}

use std::sync::Arc;

impl PlaybackCoordinator {
    fn new() -> Self {
        Self {
            active_engine: PlaybackEngineKind::Sendspin,
            sendspin: SendspinEngine::new(),
            offline_available: false,
        }
    }

    fn set_active_engine(&mut self, engine: &str) -> Result<(), String> {
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

    fn set_active_engine_configured(
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
            "offline" | "local_offline" => self.set_active_engine(&normalized_engine),
            value => Err(format!("unsupported playback engine: {value}")),
        }
    }

    fn initialize(&mut self, config_json: &str) -> Result<(), String> {
        let config: Value = serde_json::from_str(config_json)
            .map_err(|error| format!("invalid playback engine config: {error}"))?;
        self.sendspin.init(config);
        Ok(())
    }

    fn activate(&mut self) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.connect(),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn pump(&mut self) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.pump_playback();
        }
    }

    fn deactivate(&mut self) {
        if self.active_engine == PlaybackEngineKind::Sendspin {
            self.sendspin.disconnect();
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

    fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.set_preferred_codec(codec),
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

    fn play(&mut self, content_key: &str) -> Result<(), String> {
        match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.play(content_key),
            PlaybackEngineKind::Offline => {
                Err("offline playback engine is not implemented yet".to_string())
            }
        }
    }

    fn snapshot_json(&mut self) -> Result<String, String> {
        let snapshot = match self.active_engine {
            PlaybackEngineKind::Sendspin => self.sendspin.snapshot(),
            PlaybackEngineKind::Offline => self.sendspin.snapshot(),
        };
        snapshot_json(self.active_engine, snapshot, self.offline_available)
    }
}

fn snapshot_json(
    engine: PlaybackEngineKind,
    snapshot: &SendspinSnapshot,
    offline_available: bool,
) -> Result<String, String> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|error| format!("serialize playback snapshot failed: {error}"))?;
    if let Value::Object(object) = &mut value {
        object.insert("engine".to_string(), json!(engine.as_str()));
        object.insert("offlineAvailable".to_string(), json!(offline_available));
        object.insert("queue".to_string(), json!(snapshot.queue));
        object.insert("currentIndex".to_string(), json!(snapshot.current_index));
    }
    serde_json::to_string(&value).map_err(|error| format!("serialize snapshot failed: {error}"))
}

fn set_last_error(message: impl Into<String>) {
    let message = message.into();
    if let Ok(mut last_error) = LAST_ERROR.lock() {
        *last_error = message.clone();
    }
    emit_error_event(&message);
}

fn emit_error_event(message: &str) {
    emit_json(json!({
        "playbackState": "error",
        "error": message,
    }));
}

fn emit_snapshot() {
    let Ok(mut coordinator) = COORDINATOR.lock() else {
        return;
    };
    let Ok(snapshot) = coordinator.snapshot_json() else {
        return;
    };
    emit_json_text(&snapshot);
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
        let mut last_snapshot = None::<String>;
        while !stop_for_thread.load(Ordering::Relaxed) {
            let snapshot = match COORDINATOR.lock() {
                Ok(mut coordinator) => {
                    coordinator.pump();
                    coordinator.snapshot_json().ok()
                }
                Err(_) => None,
            };
            if let Some(snapshot) = snapshot {
                if last_snapshot.as_deref() != Some(snapshot.as_str()) {
                    emit_json_text(&snapshot);
                    last_snapshot = Some(snapshot);
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

fn emit_json(value: Value) {
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

fn read_utf8(pointer: *const c_char, name: &str) -> Result<String, String> {
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
    read_utf8(pointer, name).map(Some)
}

fn with_coordinator<F>(operation: F) -> i32
where
    F: FnOnce(&mut PlaybackCoordinator) -> Result<(), String>,
{
    let result = match COORDINATOR.lock() {
        Ok(mut coordinator) => {
            operation(&mut coordinator).and_then(|()| coordinator.snapshot_json())
        }
        Err(_) => Err("playback coordinator lock poisoned".to_string()),
    };
    match result {
        Ok(snapshot) => {
            emit_json_text(&snapshot);
            0
        }
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_queue_index(index: i32) -> i32 {
    if index < 0 {
        return -1;
    }
    with_coordinator(|coordinator| coordinator.play_queue_index(index as usize))
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
pub extern "C" fn mashin_playback_engine_set_active_engine(
    engine: *const c_char,
    config_json: *const c_char,
) -> i32 {
    let engine = match read_utf8(engine, "engine") {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            return -1;
        }
    };
    let config_json = match read_optional_utf8(config_json, "engine_config_json") {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            return -1;
        }
    };
    with_coordinator(|coordinator| {
        coordinator.set_active_engine_configured(&engine, config_json.as_deref())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_initialize(config_json: *const c_char) -> i32 {
    match read_utf8(config_json, "config_json") {
        Ok(config) => with_coordinator(|coordinator| coordinator.initialize(&config)),
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_activate() -> i32 {
    let result = with_coordinator(|coordinator| coordinator.activate());
    if result == 0 {
        start_playback_runtime();
    }
    result
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_deactivate() -> i32 {
    stop_playback_runtime();
    with_coordinator(|coordinator| {
        coordinator.deactivate();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media(media_items_json: *const c_char) -> i32 {
    match read_utf8(media_items_json, "media_items_json") {
        Ok(media_items) => with_coordinator(|coordinator| coordinator.play_media(&media_items)),
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

fn queue_items_command(items_json: *const c_char, replace_following: bool) -> i32 {
    match read_utf8(items_json, "media_items_json") {
        Ok(items) => {
            with_coordinator(|coordinator| coordinator.play_media_next(&items, replace_following))
        }
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media_next(items: *const c_char) -> i32 {
    queue_items_command(items, false)
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media_replace_next(items: *const c_char) -> i32 {
    queue_items_command(items, true)
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_play_media_last(items: *const c_char) -> i32 {
    match read_utf8(items, "media_items_json") {
        Ok(items) => with_coordinator(|coordinator| coordinator.play_media_last(&items)),
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_preferred_codec(codec: *const c_char) -> i32 {
    match read_utf8(codec, "codec") {
        Ok(codec) => with_coordinator(|coordinator| coordinator.set_preferred_codec(&codec)),
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_clear_queue() -> i32 {
    with_coordinator(|coordinator| coordinator.clear_queue())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_move_queue_item(
    item_id: *const c_char,
    shift: i32,
) -> i32 {
    match read_utf8(item_id, "queue_item_id") {
        Ok(item_id) => with_coordinator(|coordinator| coordinator.move_queue_item(&item_id, shift)),
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_delete_queue_item(item_id: *const c_char) -> i32 {
    match read_utf8(item_id, "queue_item_id") {
        Ok(item_id) => with_coordinator(|coordinator| coordinator.delete_queue_item(&item_id)),
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_play() -> i32 {
    with_coordinator(|coordinator| coordinator.transport_play())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_pause() -> i32 {
    with_coordinator(|coordinator| {
        coordinator.transport_pause();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_stop() -> i32 {
    with_coordinator(|coordinator| {
        coordinator.transport_stop();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_next() -> i32 {
    with_coordinator(|coordinator| coordinator.transport_next())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_previous() -> i32 {
    with_coordinator(|coordinator| coordinator.transport_previous())
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_transport_seek(seconds: f64) -> i32 {
    with_coordinator(|coordinator| coordinator.transport_seek(seconds))
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_shuffle(enabled: i32) -> i32 {
    with_coordinator(|coordinator| {
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
    with_coordinator(|coordinator| {
        coordinator.set_repeat_mode(mode);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_volume(volume: i32) -> i32 {
    with_coordinator(|coordinator| {
        coordinator.set_volume(volume.clamp(0, 100) as u8);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_set_muted(muted: i32) -> i32 {
    with_coordinator(|coordinator| {
        coordinator.set_muted(muted != 0);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_playback_engine_get_snapshot_json() -> *mut c_char {
    let Ok(mut coordinator) = COORDINATOR.lock() else {
        return ptr::null_mut();
    };
    let Ok(snapshot) = coordinator.snapshot_json() else {
        return ptr::null_mut();
    };
    CString::new(snapshot)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
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
