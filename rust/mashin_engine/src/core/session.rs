//! Application login, output selection, and transactional session persistence.

use super::{CoreCoordinator, CORE_NETWORK_RUNTIME};
use crate::core::config::{default_local_output, refresh_local_client_name, DEFAULT_LOCAL_VOLUME};
use crate::core::models::{
    LogLevelPreference, LoginConfig, SelectedOutput, SessionConfig, ThemePreference,
};
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::models::identifiers::AccessToken;

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
        let Some(mut session) = self.auth_store.load_session()? else {
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
        let access_token = access_token_from_login_response(login_response)?;
        let authenticated_client =
            MusicAssistantClient::new(&login.music_assistant_url.0, Some(access_token.clone()));
        if !CORE_NETWORK_RUNTIME.block_on(authenticated_client.has_valid_session()) {
            return Err("Music Assistant rejected the authenticated session".to_string());
        }
        self.configure_session(SessionConfig {
            music_assistant_url: login.music_assistant_url,
            access_token: Some(access_token),
            user_id: None,
            username: login.username,
            selected_output: SelectedOutput::Local,
            local: default_local_output(),
            volume: DEFAULT_LOCAL_VOLUME,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
        })
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
        if let Err(error) = self.auth_store.save_session(session.clone()) {
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
}

pub(super) fn access_token_from_login_response(
    login_response: serde_json::Value,
) -> Result<AccessToken, String> {
    if login_response
        .get("success")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return Err("Music Assistant login failed".to_string());
    }
    let token = login_response
        .get("token")
        .or_else(|| login_response.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            "Music Assistant login response did not contain an access token".to_string()
        })?;
    Ok(AccessToken(token.to_string()))
}
