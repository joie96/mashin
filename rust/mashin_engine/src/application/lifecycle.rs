//! Serializes lifecycle transitions, releases state locks before I/O, and joins retired session work.
use crate::application::errors::ApplicationError;
use crate::application::events::ApplicationEvent;
use crate::application::playback::collect_media_session_events;
use crate::application::session::RetiredWorker;
use crate::application::state::ApplicationState;
use crate::application::Application;
use crate::logging::Logger;
use std::sync::Arc;
static LOGGER: Logger = Logger::new(module_path!());
impl Application {
    // Lifecycle and session completion barriers.

    /// Drops session-owned UI data and pending favorite wishes without persisting them.
    pub(crate) fn shutdown(&self) -> Result<(), ApplicationError> {
        let _command_guard = self
            .command_serializer
            .lock()
            .map_err(|_| ApplicationError::internal("core command serializer lock poisoned"))?;
        let (shutdown_repositories,generation)={
            let mut coordinator=self.coordinator.lock()
                .map_err(|_|ApplicationError::internal("core coordinator lock poisoned"))?;
            coordinator.shutdown();
            (coordinator.environment.shutdown_repositories.clone(),coordinator.session_generation)
        };
        drop(_command_guard);
        let completion=self.join_retired_session_work();
        let cache_completion=self.runtime.block_on(shutdown_repositories(generation))
            .map_err(ApplicationError::command_failed);
        completion?;
        cache_completion
    }

    pub(crate) fn restore_persisted_session_and_collect_events(
        &self,
    ) -> Result<(bool, Vec<ApplicationEvent>), ApplicationError> {
        let _command_guard = self
            .command_serializer
            .lock()
            .map_err(|_| ApplicationError::internal("core command serializer lock poisoned"))?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        let queue_before = coordinator.playback.queue();
        let player_state_before = coordinator.playback.player_state();
        let restored = coordinator
            .restore_persisted_session()
            .map_err(ApplicationError::command_failed);
        let should_load_session_data =
            restored.as_ref().is_ok_and(|restored| *restored) && coordinator.user_data.is_none();
        let accepted_generation = coordinator.session_generation;
        let events = collect_media_session_events(
            coordinator.media_session_state(),
            queue_before,
            player_state_before,
        );
        drop(coordinator);
        drop(_command_guard);
        let completion = self.join_retired_session_work();
        let has_restored_session = restored?;
        completion?;
        if should_load_session_data {
            self.start_session_data_load(accepted_generation);
        }
        Ok((has_restored_session, events))
    }

    // Commands and transport.

    // Commands publish only the projections whose values changed under the command lock.
    pub(crate) fn execute_command_and_collect_events<F>(
        &self,
        operation: F,
    ) -> Result<Vec<ApplicationEvent>, ApplicationError>
    where
        F: FnOnce(&mut ApplicationState) -> Result<(), String>,
    {
        let _command_guard = self
            .command_serializer
            .lock()
            .map_err(|_| ApplicationError::internal("core command serializer lock poisoned"))?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        let queue_before = coordinator.playback.queue();
        let player_state_before = coordinator.playback.player_state();
        let session_generation = coordinator.session_generation;
        let result = operation(&mut coordinator);
        let commands = coordinator
            .take_pending_commands()
            .map_err(ApplicationError::command_failed);
        let should_load_session_data = coordinator.session_generation != session_generation
            && coordinator.session_configuration.is_some();
        let accepted_generation = coordinator.session_generation;
        drop(coordinator);
        drop(_command_guard);
        let completion = self.join_retired_session_work();
        let commands = commands?;
        result.map_err(ApplicationError::command_failed)?;
        completion?;
        for command in commands {
            self.runtime
                .block_on(command)
                .map_err(ApplicationError::command_failed)?;
        }
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        let events = collect_media_session_events(
            coordinator.media_session_state(),
            queue_before,
            player_state_before,
        );
        drop(coordinator);
        if should_load_session_data {
            self.start_session_data_load(accepted_generation);
        }
        Ok(events)
    }

    // Session worker coordination.

    pub(super) fn start_session_data_load(&self, expected_generation: u64) {
        let Ok(context) = self.query_context() else {
            return;
        };
        if context.generation != expected_generation {
            return;
        }
        let tasks = context.tasks.clone();
        let application = self.clone();
        let runtime = self.runtime.clone();
        let _ = context.tasks.spawn(&self.runtime, async move {
            let result = tasks
                .run_blocking(&runtime, move || {
                    application.read_or_load_session_data(expected_generation)
                })
                .await;
            if let Err(error) = result.and_then(|result| result) {
                LOGGER.warn(format!("load session data failed: {}", error.message));
            }
        });
    }

    fn join_retired_session_work(&self) -> Result<(), ApplicationError> {
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        let workers = coordinator.playback.take_retired_workers();
        coordinator.retired_workers.extend(
            workers
                .into_iter()
                .map(|worker| Arc::new(RetiredWorker::new(worker))),
        );
        if let Some(connection) = &coordinator.connection {
            let workers = connection
                .event_hub
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take_retired_workers();
            coordinator.retired_workers.extend(
                workers
                    .into_iter()
                    .map(|worker| Arc::new(RetiredWorker::new(worker))),
            );
        }
        let retired = coordinator.retired_session_tasks.clone();
        let retired_workers = coordinator.retired_workers.clone();
        drop(coordinator);
        if retired.is_empty() && retired_workers.is_empty() {
            return Ok(());
        }
        let result = self.runtime.block_on(async {
            let mut failure = None;
            for tasks in &retired {
                if let Err(error) = tasks.join().await {
                    failure.get_or_insert(error);
                }
            }
            for worker in &retired_workers {
                if let Err(error) = worker.join().await {
                    failure.get_or_insert(error);
                }
            }

            failure.map_or(Ok(()), Err)
        });
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        coordinator
            .retired_session_tasks
            .retain(|tasks| !retired.iter().any(|joined| Arc::ptr_eq(tasks, joined)));
        coordinator.retired_workers.retain(|worker| {
            !retired_workers
                .iter()
                .any(|joined| Arc::ptr_eq(worker, joined))
        });

        result
    }
}
