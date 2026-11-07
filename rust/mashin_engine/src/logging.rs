use chrono::{DateTime, Local};
use once_cell::sync::Lazy;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

static MINIMUM_LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);
static LOG_FILE: Lazy<Mutex<DailyLogFile>> = Lazy::new(|| Mutex::new(DailyLogFile::default()));
static LOG_EVENT_SINK: OnceLock<fn(&str)> = OnceLock::new();
const MAX_LOG_FILES: usize = 3;

// Owns the process-wide append handle; callers serialize writes and rotation with LOG_FILE.
#[derive(Default)]
struct DailyLogFile {
    directory_resolver: Option<fn() -> Result<PathBuf, String>>,
    active_file: Option<(PathBuf, File)>,
    has_started: bool,
}

impl DailyLogFile {
    // Persistent output and daily rotation.
    fn append_current(&mut self, line: &str) -> Result<PathBuf, String> {
        let directory = self
            .active_file
            .as_ref()
            .and_then(|(path, _)| path.parent().map(Path::to_path_buf))
            .map(Ok)
            .unwrap_or_else(|| {
                self.directory_resolver
                    .ok_or_else(|| "log directory is not initialized".to_string())?(
                )
            })?;
        self.append(&directory, Local::now(), line)
            .map_err(|error| format!("write daily log failed: {error}"))
    }

    fn append(
        &mut self,
        directory: &Path,
        timestamp: DateTime<Local>,
        line: &str,
    ) -> io::Result<PathBuf> {
        let path = directory.join(format!("mashin-{}.log", timestamp.format("%Y-%m-%d")));
        if self.active_file.as_ref().map(|(path, _)| path) != Some(&path) {
            fs::create_dir_all(directory)?;
            let mut file = OpenOptions::new().append(true).create(true).open(&path)?;
            if !self.has_started {
                writeln!(
                    file,
                    "\n========== App Start {} ==========",
                    timestamp.format("%Y-%m-%d %H:%M:%S")
                )?;
            }
            self.active_file = None;
            Self::prune(directory, &path)?;
            self.active_file = Some((path.clone(), file));
            self.has_started = true;
        }
        if !line.is_empty() {
            if let Some((_, file)) = &mut self.active_file {
                writeln!(file, "{line}")?;
            }
        }
        Ok(path)
    }

    fn prune(directory: &Path, active_path: &Path) -> io::Result<()> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type()?.is_file()
                && name.starts_with("mashin-")
                && name.ends_with(".log")
                && entry.path() != active_path
            {
                paths.push(entry.path());
            }
        }
        paths.sort();
        let excess = paths.len().saturating_sub(MAX_LOG_FILES - 1);
        for path in paths.into_iter().take(excess) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

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

// Logging failures must not prevent startup; subsequent writes retry initialization.
pub(crate) fn initialize(
    directory_resolver: fn() -> Result<PathBuf, String>,
    event_sink: fn(&str),
) {
    LOG_EVENT_SINK.get_or_init(|| event_sink);
    let result = LOG_FILE
        .lock()
        .map_err(|_| "log file lock poisoned".to_string())
        .and_then(|mut log_file| {
            log_file
                .directory_resolver
                .get_or_insert(directory_resolver);
            log_file.append_current("")
        });
    if let Err(error) = result {
        let _ = writeln!(io::stderr().lock(), "Mashin log persistence: {error}");
    }
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
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S%.3f");
    let text = format!(
        "[{timestamp}] [{}] {}: {message}",
        level.short_name(),
        category,
    );
    let persistence = (|| {
        LOG_FILE
            .lock()
            .map_err(|_| "log file lock poisoned".to_string())?
            .append_current(&text)
    })();
    if let Err(error) = persistence {
        let _ = writeln!(io::stderr().lock(), "Mashin log persistence: {error}");
    }
    #[cfg(target_os = "android")]
    android_log(level, &text);
    let _ = writeln!(io::stderr().lock(), "{text}");
    if let Some(event_sink) = LOG_EVENT_SINK.get() {
        event_sink(&text);
    }
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
