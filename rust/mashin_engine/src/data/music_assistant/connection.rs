//! Shared authenticated API resources for one application session.
//! Receiver sockets use only the server URL, never the API token.

use super::event_hub::MusicAssistantEventHub;
use crate::data::music_assistant::client::MusicAssistantClient;
use crate::domain::session::{AccessToken, MusicAssistantServerUrl};
use std::sync::{Arc, Mutex};

pub(crate) struct MusicAssistantConnection {
    pub(crate) server_url: MusicAssistantServerUrl,
    pub(crate) access_token: Option<AccessToken>,
    pub(crate) client: Arc<MusicAssistantClient>,
    pub(crate) event_hub: Arc<Mutex<MusicAssistantEventHub>>,
}

impl MusicAssistantConnection {
    // One API client and EventHub are shared by both playback targets.
    pub(crate) fn new(
        server_url: MusicAssistantServerUrl,
        access_token: Option<AccessToken>,
    ) -> Result<Self, String> {
        let event_hub = Arc::new(Mutex::new(MusicAssistantEventHub::new(
            &server_url,
            access_token.as_ref(),
        )?));
        Ok(Self {
            client: Arc::new(MusicAssistantClient::new(
                &server_url.0,
                access_token.clone(),
            )),
            server_url,
            access_token,
            event_hub,
        })
    }

    // Credentials are compared here, not duplicated in local device configuration.
    pub(crate) fn matches(
        &self,
        server_url: &MusicAssistantServerUrl,
        access_token: &Option<AccessToken>,
    ) -> bool {
        self.server_url == *server_url && self.access_token == *access_token
    }
}
