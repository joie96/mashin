//! Native composition root; business code receives an application instance, not this global.

use crate::application::events::EventPublisher;
use crate::application::Application;
use once_cell::sync::Lazy;
use runtime::ApplicationRuntime;

mod runtime;
pub(crate) mod session;

static APPLICATION: Lazy<ApplicationRuntime> = Lazy::new(|| {
    ApplicationRuntime::new(
        session::environment(),
        EventPublisher::new(crate::adapters::event_router::publish_background_event),
        session::create_session_storage(),
    )
    .expect("create native application")
});

// Only native entry points resolve the process application here.
pub(crate) fn application() -> &'static Application {
    APPLICATION.application()
}

pub(crate) fn initialize_logging() {
    crate::logging::initialize(log_directory, crate::adapters::event_router::publish_log);
}

// Resolve on demand so Android can retry after its application context becomes available.
fn log_directory() -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    return Ok(std::env::temp_dir().join(format!("mashin-test-logs-{}", std::process::id())));
    #[cfg(all(not(test), any(target_os = "macos", target_os = "ios")))]
    return std::env::var_os("HOME")
        .map(|path| {
            std::path::PathBuf::from(path)
                .join("Library")
                .join("Logs")
                .join("Mashin")
        })
        .ok_or_else(|| "HOME is not set".to_string());
    #[cfg(all(not(test), not(any(target_os = "macos", target_os = "ios"))))]
    session::application_data_directory().map(|path| {
        path.join(if cfg!(target_os = "linux") {
            "logs"
        } else {
            "Logs"
        })
    })
}
