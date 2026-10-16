//! Shared authenticated API resources for one application session.
//! Receiver sockets use only the server URL, never the API token.

use super::client::MusicAssistantClient;
use super::event_hub::MusicAssistantEventHub;
use super::models::identifiers::AccessToken;
use super::models::session::MusicAssistantServerUrl;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_identity_includes_server_and_credentials() {
        let server_url = MusicAssistantServerUrl("http://127.0.0.1:1".into());
        let access_token = Some(AccessToken("test-token".into()));
        let connection = Arc::new(
            MusicAssistantConnection::new(server_url.clone(), access_token.clone()).unwrap(),
        );
        let target = connection.clone();
        assert!(connection.matches(&server_url, &access_token));
        assert!(!connection.matches(
            &MusicAssistantServerUrl("http://other:8095".into()),
            &access_token
        ));
        assert!(!connection.matches(&server_url, &None));
        assert!(Arc::ptr_eq(&connection.client, &target.client));
        assert!(Arc::ptr_eq(&connection.event_hub, &target.event_hub));
    }

    #[test]
    fn connection_requires_valid_server_and_credentials() {
        let server_url = MusicAssistantServerUrl("http://127.0.0.1:1".into());
        assert!(MusicAssistantConnection::new(server_url.clone(), None).is_err());
        assert!(MusicAssistantConnection::new(server_url, Some(AccessToken("  ".into()))).is_err());
        assert!(MusicAssistantConnection::new(
            MusicAssistantServerUrl("not a URL".into()),
            Some(AccessToken("token".into()))
        )
        .is_err());
    }
}
