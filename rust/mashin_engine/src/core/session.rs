//! Application login, output selection, and transactional session persistence.

use super::{CoreCoordinator, CORE_NETWORK_RUNTIME};
use crate::core::config::{default_local_output, refresh_local_client_name, DEFAULT_LOCAL_VOLUME};
use crate::core::models::{
    LogLevelPreference, LoginConfig, SelectedOutput, SessionConfig, ThemePreference,
};
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::models::identifiers::{AccessToken, UserId};
use serde::Deserialize;

#[derive(Deserialize)]
struct LoginIdentity {
    access_token: AccessToken,
    user: LoginUser,
}

#[derive(Deserialize)]
struct LoginUser {
    user_id: UserId,
    username: String,
}

impl CoreCoordinator {
    // Session configuration, persistence, and output selection.
    pub(crate) fn configure_session(&mut self, mut session: SessionConfig) -> Result<(), String> {
        refresh_local_client_name(&mut session.local);
        self.configure_and_persist_session(&session)?;
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.invalidate_user_data();
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn restore_persisted_session(&mut self) -> Result<bool, String> {
        if self.session_configuration.is_some() {
            return Ok(true);
        }
        let Some(mut session) = self.session_store.load_session()? else {
            return Ok(false);
        };
        refresh_local_client_name(&mut session.local);
        self.configure_selected_output(&session)?;
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.invalidate_user_data();
        self.session_configuration = Some(session);
        Ok(true)
    }

    pub(crate) fn authenticate_and_configure_session(
        &mut self,
        login: LoginConfig,
    ) -> Result<(), String> {
        if login.music_assistant_url.0.trim().is_empty() {
            return Err("Music Assistant URL is required".to_string());
        }
        if login.username.trim().is_empty() {
            return Err("Music Assistant username is required".to_string());
        }
        if login.password.is_empty() {
            return Err("Music Assistant password is required".to_string());
        }
        let unauthenticated_client = MusicAssistantClient::new(&login.music_assistant_url.0, None);
        let login_response = CORE_NETWORK_RUNTIME
            .block_on(unauthenticated_client.login(&login.username, &login.password))?;
        let identity = identity_from_login_response(login_response)?;
        let access_token = identity.access_token;
        let authenticated_client =
            MusicAssistantClient::new(&login.music_assistant_url.0, Some(access_token.clone()));
        if !CORE_NETWORK_RUNTIME.block_on(authenticated_client.has_valid_session()) {
            return Err("Music Assistant rejected the authenticated session".to_string());
        }
        self.configure_session(SessionConfig {
            music_assistant_url: login.music_assistant_url,
            access_token: Some(access_token),
            user_id: Some(identity.user.user_id),
            username: identity.user.username,
            selected_output: SelectedOutput::Local,
            local: default_local_output(),
            volume: DEFAULT_LOCAL_VOLUME,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
            should_show_multi_device_button: true,
            preferred_provider: None,
        })
    }

    // Local logout remains effective when the server cannot revoke its token.
    pub(crate) fn logout(&mut self) -> Result<(), String> {
        self.session_store.clear_session()?;
        let connection = self.connection.clone();
        self.shutdown();
        if let Some(connection) = connection {
            let _ = CORE_NETWORK_RUNTIME.block_on(connection.client.logout());
        }
        Ok(())
    }

    pub(crate) fn select_output(&mut self, selected_output: SelectedOutput) -> Result<(), String> {
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before selecting an output".to_string()
        })?;
        session.selected_output = selected_output;
        self.configure_and_persist_session(&session)?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn select_local_output(&mut self) -> Result<(), String> {
        self.select_output(SelectedOutput::Local)
    }

    pub(super) fn configure_and_persist_session(
        &mut self,
        session: &SessionConfig,
    ) -> Result<(), String> {
        let previous = self.session_configuration.clone();
        let previous_playback = self.playback.checkpoint();
        let previous_connection = self.connection.clone();
        let requires_configuration = previous.as_ref().is_none_or(|current| {
            current.selected_output != session.selected_output
                || current.music_assistant_url != session.music_assistant_url
                || current.access_token != session.access_token
                || current.local != session.local
        });
        if requires_configuration {
            self.configure_selected_output(session)?;
        }
        if let Err(error) = self.session_store.save_session(session.clone()) {
            if requires_configuration {
                self.playback
                    .restore_session(previous_playback, previous_connection.clone())?;
                self.connection = previous_connection;
            }
            return Err(error);
        }
        Ok(())
    }

    // Validate and prepare replacements before releasing the currently owned session.
    fn configure_selected_output(&mut self, session: &SessionConfig) -> Result<(), String> {
        if matches!(&session.selected_output, SelectedOutput::Remote(remote) if remote.player_id.as_str().trim().is_empty())
        {
            return Err("Music Assistant remote player ID is required".to_string());
        }
        let connection = match self.connection.as_ref().filter(|connection| {
            connection.matches(&session.music_assistant_url, &session.access_token)
        }) {
            Some(connection) => connection.clone(),
            None => std::sync::Arc::new(MusicAssistantConnection::new(
                session.music_assistant_url.clone(),
                session.access_token.clone(),
            )?),
        };
        if let Some(user_id) = &session.user_id {
            connection
                .client
                .initialize_current_user_id(user_id.clone())?;
        }
        self.playback.configure_session(
            crate::playback::models::PlaybackSessionConfiguration {
                local: crate::playback::local::session::LocalSessionConfiguration {
                    device_id: session.local.device_id.clone(),
                    client_name: session.local.client_name.clone(),
                    cache_bytes: session.local.cache_bytes,
                    audio_profile: session.local.audio_profile,
                    volume: session.volume,
                },
                target: match &session.selected_output {
                    SelectedOutput::Local => crate::playback::models::PlaybackTarget::Local,
                    SelectedOutput::Remote(remote) => {
                        crate::playback::models::PlaybackTarget::Remote(remote.player_id.clone())
                    }
                },
            },
            connection.clone(),
            &CORE_NETWORK_RUNTIME,
        )?;
        self.connection = Some(connection);
        Ok(())
    }

    // Persist identity enrichment only for the session that requested it.
    pub(super) fn remember_current_user_id(
        &mut self,
        generation: u64,
        user_id: UserId,
    ) -> Result<(), String> {
        if self.user_data_session_generation != generation {
            return Err("session changed while resolving the current user".into());
        }
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required".to_owned())?;
        if let Some(current) = &session.user_id {
            return if current == &user_id {
                Ok(())
            } else {
                Err("Music Assistant session user ID cannot change".into())
            };
        }
        session.user_id = Some(user_id);
        self.session_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }
}

fn identity_from_login_response(
    login_response: serde_json::Value,
) -> Result<LoginIdentity, String> {
    if login_response
        .get("success")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return Err("Music Assistant login failed".to_string());
    }
    let identity: LoginIdentity = serde_json::from_value(login_response)
        .map_err(|error| format!("invalid Music Assistant login response: {error}"))?;
    if identity.access_token.as_str().trim().is_empty()
        || identity.user.user_id.as_str().trim().is_empty()
        || identity.user.username.trim().is_empty()
    {
        return Err("Music Assistant login response contains an empty session identity".into());
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn login_identity_preserves_server_user_id_and_username() {
        let identity = identity_from_login_response(json!({
            "success":true, "access_token":"token",
            "user":{"user_id":"user-1", "username":"canonical-name", "role":"user"}
        }))
        .unwrap();
        assert_eq!(identity.access_token.as_str(), "token");
        assert_eq!(identity.user.user_id.as_str(), "user-1");
        assert_eq!(identity.user.username, "canonical-name");
    }

    #[test]
    fn login_identity_rejects_failed_missing_and_empty_identities() {
        for response in [
            json!({"success":false, "error":"denied"}),
            json!({"success":true, "access_token":"token"}),
            json!({"access_token":"token", "user":{"user_id":"", "username":"test"}}),
            json!({"access_token":"", "user":{"user_id":"user-1", "username":"test"}}),
        ] {
            assert!(identity_from_login_response(response).is_err());
        }
    }
}
