use chrono::{DateTime, Local};
use once_cell::sync::Lazy;
use std::collections::VecDeque;
use std::env;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

static MINIMUM_LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);
const LOG_CAPACITY: usize = 10_000;
static LOG_LINES: Lazy<Mutex<VecDeque<String>>> = Lazy::new(|| Mutex::new(VecDeque::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn short_name(self) -> &'static str {
        match self {
            Self::Trace => "TRC",
            Self::Debug => "DBG",
            Self::Info => "INF",
            Self::Warn => "WRN",
            Self::Error => "ERR",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

pub(crate) fn set_minimum_level(value: &str) -> Result<(), String> {
    let level = LogLevel::parse(value).ok_or_else(|| {
        format!("invalid log level: {value}; expected trace, debug, info, warn, or error")
    })?;
    MINIMUM_LOG_LEVEL.store(level as u8, Ordering::Relaxed);
    Ok(())
}

pub(crate) fn log_external(level: &str, category: &str, message: &str) -> Result<(), String> {
    let level = LogLevel::parse(level).ok_or_else(|| {
        format!("invalid log level: {level}; expected trace, debug, info, warn, or error")
    })?;
    emit_log(level, category, message);
    Ok(())
}

// Export a snapshot without holding the logger lock across filesystem I/O.
pub(crate) fn save_log_snapshot() -> Result<String, String> {
    let lines: Vec<String> = LOG_LINES
        .lock()
        .map_err(|_| "log buffer lock poisoned".to_string())?
        .iter()
        .cloned()
        .collect();
    if lines.is_empty() {
        return Err("no log entries to save".to_string());
    }
    let directory = log_directory()?;
    let path = write_log_snapshot(&lines, &directory, Local::now())?;
    Ok(path.to_string_lossy().into_owned())
}

fn write_log_snapshot(
    lines: &[String],
    directory: &Path,
    timestamp: DateTime<Local>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|error| {
        format!(
            "create log directory {} failed: {error}",
            directory.display()
        )
    })?;
    let path = directory.join(format!(
        "mashin-{}.log",
        timestamp.format("%Y-%m-%d_%H-%M-%S-%6f")
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("create log file {} failed: {error}", path.display()))?;
    for line in lines {
        writeln!(file, "{line}")
            .map_err(|error| format!("write log file {} failed: {error}", path.display()))?;
    }
    Ok(path)
}

#[cfg(target_os = "android")]
fn log_directory() -> Result<PathBuf, String> {
    crate::jni::android_context::external_application_files_directory()
        .map(|path| path.join("Logs"))
}

#[cfg(target_os = "windows")]
fn log_directory() -> Result<PathBuf, String> {
    env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Mashin").join("Logs"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn log_directory() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(|path| {
            PathBuf::from(path)
                .join("Library")
                .join("Logs")
                .join("Mashin")
        })
        .ok_or_else(|| "HOME is not set".to_string())
}

#[cfg(target_os = "linux")]
fn log_directory() -> Result<PathBuf, String> {
    let state_directory = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/state")))
        .ok_or_else(|| "neither XDG_STATE_HOME nor HOME is set".to_string())?;
    Ok(state_directory.join("mashin").join("logs"))
}

pub(crate) struct Logger {
    category: &'static str,
}

impl Logger {
    pub(crate) const fn new(category: &'static str) -> Self {
        Self { category }
    }

    pub(crate) fn debug(&self, message: impl fmt::Display) {
        self.log(LogLevel::Debug, message);
    }

    pub(crate) fn trace(&self, message: impl fmt::Display) {
        self.log(LogLevel::Trace, message);
    }

    pub(crate) fn info(&self, message: impl fmt::Display) {
        self.log(LogLevel::Info, message);
    }

    pub(crate) fn warn(&self, message: impl fmt::Display) {
        self.log(LogLevel::Warn, message);
    }

    pub(crate) fn error(&self, message: impl fmt::Display) {
        self.log(LogLevel::Error, message);
    }

    fn log(&self, level: LogLevel, message: impl fmt::Display) {
        emit_log(level, self.category, &message.to_string());
    }
}

fn emit_log(level: LogLevel, category: &str, message: &str) {
    if (level as u8) < MINIMUM_LOG_LEVEL.load(Ordering::Relaxed) {
        return;
    }

    let message = message.replace('\n', " ");
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let text = format!(
        "[{timestamp}] [{}] {}: {message}",
        level.short_name(),
        category,
    );
    if let Ok(mut lines) = LOG_LINES.lock() {
        retain_log_line(&mut lines, text.clone());
    }
    #[cfg(target_os = "android")]
    android_log(level, &text);
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{text}");
    crate::ffi::event_channel::publish_flutter_log_event(&text);
}

fn retain_log_line(lines: &mut VecDeque<String>, text: String) {
    if lines.len() == LOG_CAPACITY {
        lines.pop_front();
    }
    lines.push_back(text);
}

#[cfg(target_os = "android")]
fn android_log(level: LogLevel, message: &str) {
    use std::ffi::CString;

    const ANDROID_LOG_DEBUG: i32 = 3;
    const ANDROID_LOG_INFO: i32 = 4;
    const ANDROID_LOG_WARN: i32 = 5;
    const ANDROID_LOG_ERROR: i32 = 6;
    let priority = match level {
        LogLevel::Trace | LogLevel::Debug => ANDROID_LOG_DEBUG,
        LogLevel::Info => ANDROID_LOG_INFO,
        LogLevel::Warn => ANDROID_LOG_WARN,
        LogLevel::Error => ANDROID_LOG_ERROR,
    };
    let Ok(tag) = CString::new("MashinRust") else {
        return;
    };
    let Ok(message) = CString::new(message) else {
        return;
    };
    unsafe {
        __android_log_write(priority, tag.as_ptr(), message.as_ptr());
    }
}

#[cfg(target_os = "android")]
#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(
        priority: i32,
        tag: *const std::ffi::c_char,
        text: *const std::ffi::c_char,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::{
        log_external, retain_log_line, set_minimum_level, write_log_snapshot, LogLevel,
        LOG_CAPACITY, MINIMUM_LOG_LEVEL,
    };
    use chrono::{Local, TimeZone};
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::atomic::Ordering;

    #[test]
    fn uses_flutter_level_names() {
        assert_eq!(LogLevel::Trace.short_name(), "TRC");
        assert_eq!(LogLevel::Debug.short_name(), "DBG");
        assert_eq!(LogLevel::Info.short_name(), "INF");
        assert_eq!(LogLevel::Warn.short_name(), "WRN");
        assert_eq!(LogLevel::Error.short_name(), "ERR");
    }

    #[test]
    fn accepts_shared_log_level_names() {
        set_minimum_level("trace").unwrap();
        assert_eq!(
            MINIMUM_LOG_LEVEL.load(Ordering::Relaxed),
            LogLevel::Trace as u8
        );
        assert!(set_minimum_level("verbose").is_err());
    }

    #[test]
    fn accepts_flutter_log_messages() {
        set_minimum_level("trace").unwrap();

        assert!(log_external("info", "Flutter", "widget updated").is_ok());
        assert!(log_external("verbose", "Flutter", "widget updated").is_err());
    }

    #[test]
    fn retains_only_the_newest_log_lines() {
        let mut lines = VecDeque::new();
        for index in 0..=LOG_CAPACITY {
            retain_log_line(&mut lines, format!("line {index}"));
        }
        assert_eq!(lines.len(), LOG_CAPACITY);
        assert_eq!(lines.front().unwrap(), "line 1");
        assert_eq!(lines.back().unwrap(), "line 10000");
    }

    #[test]
    fn writes_a_dated_snapshot_without_overwriting_it() {
        let directory =
            std::env::temp_dir().join(format!("mashin-log-test-{}", std::process::id()));
        let timestamp = Local
            .with_ymd_and_hms(2026, 9, 23, 15, 4, 5)
            .single()
            .unwrap();
        let lines = vec!["first".to_string(), "second".to_string()];
        let path = write_log_snapshot(&lines, &directory, timestamp).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "mashin-2026-09-23_15-04-05-000000.log"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
        assert!(write_log_snapshot(&lines, &directory, timestamp).is_err());
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
