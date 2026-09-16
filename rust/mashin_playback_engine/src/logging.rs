use chrono::Local;
use std::fmt;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU8, Ordering};

static MINIMUM_LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

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
        if (level as u8) < MINIMUM_LOG_LEVEL.load(Ordering::Relaxed) {
            return;
        }

        let message = message.to_string().replace('\n', " ");
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let text = format!(
            "[{timestamp}] [{}] {}: {message}",
            level.short_name(),
            self.category,
        );
        #[cfg(target_os = "android")]
        android_log(level, &text);
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr, "{text}");
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

#[cfg(test)]
mod tests {
    use super::{set_minimum_level, LogLevel, MINIMUM_LOG_LEVEL};
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
}
