//! Owns the process executor; application clones receive only its non-owning handle.
//! Runtime destruction cancels async tasks and waits for remaining blocking work.

use crate::application::events::EventPublisher;
use crate::application::session::SessionStorage;
use crate::application::Application;
use tokio::runtime::Runtime;

pub(crate) struct ApplicationRuntime {
    application: Application,
    _executor: Runtime,
}

impl ApplicationRuntime {
    // Construction and dependency injection.
    pub(crate) fn new(
        environment: crate::application::session::SessionEnvironment,
        event_publisher: EventPublisher,
        session_storage: Box<dyn SessionStorage>,
    ) -> Result<Self, String> {
        let executor = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("create application runtime: {error}"))?;
        let application = Application::new(
            environment,
            executor.handle().clone(),
            event_publisher,
            session_storage,
        );
        Ok(Self {
            application,
            _executor: executor,
        })
    }

    // The executor outlives every application reference borrowed from this owner.
    pub(crate) fn application(&self) -> &Application {
        &self.application
    }
}
