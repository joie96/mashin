//! Application failures independent of native error retention and delivery.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ApplicationErrorCode {
    InvalidArgument,
    CommandFailed,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum ApplicationErrorSource {
    #[serde(rename = "ffi")]
    NativeAdapter,
    #[serde(rename = "core")]
    Application,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplicationError {
    pub(crate) code: ApplicationErrorCode,
    pub(crate) message: String,
    pub(crate) is_retryable: bool,
    pub(crate) source: ApplicationErrorSource,
}

impl ApplicationError {
    // Error construction.
    pub(crate) fn command_failed(message: impl Into<String>) -> Self {
        Self {
            code: ApplicationErrorCode::CommandFailed,
            message: message.into(),
            is_retryable: false,
            source: ApplicationErrorSource::Application,
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ApplicationErrorCode::Internal,
            message: message.into(),
            is_retryable: false,
            source: ApplicationErrorSource::Application,
        }
    }
}

impl From<String> for ApplicationError {
    fn from(message: String) -> Self {
        Self {
            code: ApplicationErrorCode::InvalidArgument,
            message,
            is_retryable: false,
            source: ApplicationErrorSource::NativeAdapter,
        }
    }
}
