//! Session authentication values shared by storage and server boundaries.

use super::identifiers::string_identifier;
use serde::{Deserialize, Serialize};

string_identifier!(AccessToken);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct MusicAssistantServerUrl(pub(crate) String);
