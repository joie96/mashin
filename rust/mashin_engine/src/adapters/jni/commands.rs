//! Typed Android commands into the Rust-owned playback core.
//! JNI callers refresh their snapshot synchronously; events also notify Flutter.

use crate::adapters::event_router::publish_jni_command_events;
use crate::adapters::models::decode_command;
use crate::application::commands::ApplicationCommand;
use crate::application::dispatcher::CommandValue;
use crate::application::errors::ApplicationError;
use crate::application::events::ApplicationEvent;

// Session restoration precedes snapshot synchronization in the service.
pub(crate) fn restore_persisted_session() -> Result<(bool, Vec<ApplicationEvent>), ApplicationError>
{
    let result = crate::bootstrap::application()
        .execute_command(ApplicationCommand::RestorePersistedSession {})?;
    match result.value {
        CommandValue::RestoredSession(restored) => Ok((restored, result.events)),
        _ => Err(ApplicationError::internal(
            "unexpected session restoration result",
        )),
    }
}

// JNI command validation and execution.
pub(crate) fn execute_command(kind: &str, argument: Option<&str>) -> Result<(), String> {
    let command = decode_command(kind, argument)?;
    crate::bootstrap::application()
        .execute_command(command)
        .map(|result| publish_jni_command_events(result.events))
        .map_err(|error| error.message)
}
