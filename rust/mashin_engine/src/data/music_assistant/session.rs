//! MA session and online provider endpoints without retained provider observations.

use super::dto::session::{MusicAssistantUser, ServerInfo};
use crate::application::session::AuthenticatedIdentity;
use crate::data::music_assistant::client::MusicAssistantClient;
use crate::domain::identifiers::UserId;
use crate::domain::providers::{ProviderInstance, ProviderManifest};
use crate::domain::session::AccessToken;
use crate::domain::settings::LoginConfig;
use serde::Deserialize;
use serde_json::{json, Value};

impl crate::application::repositories::ProviderClient for MusicAssistantClient {
    fn get_manifests(
        &self,
    ) -> futures_util::future::BoxFuture<'_, Result<Vec<ProviderManifest>, String>> {
        Box::pin(load_provider_manifests(self))
    }
    fn get_instances(
        &self,
    ) -> futures_util::future::BoxFuture<'_, Result<Vec<ProviderInstance>, String>> {
        Box::pin(load_provider_instances(self))
    }
    fn get_icon_data<'request>(
        &'request self,
        provider: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Option<String>, String>> {
        Box::pin(load_provider_icon_data(self, provider))
    }
}

// Authentication and session validation.
pub(crate) async fn authenticate(login: LoginConfig) -> Result<AuthenticatedIdentity, String> {
    let client = MusicAssistantClient::new(&login.music_assistant_url.0, None);
    let response = client.login(&login.username, &login.password).await?;
    let identity = identity_from_login_response(response)?;
    let client = MusicAssistantClient::new(
        &login.music_assistant_url.0,
        Some(identity.access_token.clone()),
    );
    if !client.has_valid_session().await {
        return Err("Music Assistant rejected the authenticated session".into());
    }
    Ok(AuthenticatedIdentity {
        access_token: identity.access_token,
        user_id: identity.user.user_id,
        username: identity.user.username,
    })
}

// Typed provider endpoint reads.
pub(crate) async fn load_provider_manifests(
    client: &MusicAssistantClient,
) -> Result<Vec<ProviderManifest>, String> {
    client
        .execute_command("providers/manifests", json!({}))
        .await
}

pub(crate) async fn load_provider_instances(
    client: &MusicAssistantClient,
) -> Result<Vec<ProviderInstance>, String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.execute_command("providers", json!({})),
    )
    .await
    .map_err(|_| "provider availability read timed out".to_owned())?
}

pub(crate) async fn load_provider_icon_data(
    client: &MusicAssistantClient,
    provider: &str,
) -> Result<Option<String>, String> {
    let response: Value = client
        .execute_command(
            "providers/icon",
            json!({"provider":provider,"variant":"default"}),
        )
        .await?;
    match response {
        Value::Null => Ok(None),
        Value::String(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_owned())),
        _ => Err("provider icon response must be a string or null".into()),
    }
}

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

fn identity_from_login_response(response: serde_json::Value) -> Result<LoginIdentity, String> {
    if response.get("success").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err("Music Assistant login failed".into());
    }
    let identity: LoginIdentity = serde_json::from_value(response)
        .map_err(|error| format!("invalid Music Assistant login response: {error}"))?;
    if identity.access_token.as_str().trim().is_empty()
        || identity.user.user_id.as_str().trim().is_empty()
        || identity.user.username.trim().is_empty()
    {
        return Err("Music Assistant login response contains an empty session identity".into());
    }
    Ok(identity)
}

impl MusicAssistantClient {
    // Session, server, and provider queries.
    pub(crate) async fn login(&self, username: &str, password: &str) -> Result<Value, String> {
        self.execute_command(
            "auth/login",
            json!({
                "username": username,
                "password": password,
                "provider_id": "builtin",
            }),
        )
        .await
    }

    pub(crate) async fn logout(&self) -> Result<(), String> {
        self.execute_command::<Value>("auth/logout", json!({}))
            .await
            .map(|_| ())
    }

    pub(crate) async fn get_server_info(&self) -> Result<ServerInfo, String> {
        self.execute_command("info", json!({})).await
    }

    pub(crate) async fn has_valid_session(&self) -> bool {
        self.get_server_info().await.is_ok()
    }

    pub(crate) async fn get_current_user(&self) -> Result<MusicAssistantUser, String> {
        self.execute_command("auth/me", json!({})).await
    }

    pub(crate) fn initialize_current_user_id(&self, user_id: UserId) -> Result<(), String> {
        if user_id.as_str().trim().is_empty() {
            return Err("Music Assistant user ID is empty".into());
        }
        if self.current_user_id.get() == Some(&user_id) {
            return Ok(());
        }
        self.current_user_id
            .set(user_id)
            .map_err(|_| "Music Assistant session user ID cannot change".into())
    }

    pub(crate) async fn get_current_user_id(&self) -> Result<UserId, String> {
        self.current_user_id
            .get_or_try_init(|| async {
                let user = self.get_current_user().await?;
                if user.user_id.as_str().trim().is_empty() {
                    return Err("Music Assistant user ID is empty".to_owned());
                }
                Ok(user.user_id)
            })
            .await
            .cloned()
    }
}
