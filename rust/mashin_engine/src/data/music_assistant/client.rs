use super::request_scheduler::{command_read_key, RequestScheduler};
use crate::application::repositories::{with_request_priority, RequestPriority};

use crate::domain::identifiers::UserId;
use crate::domain::session::AccessToken;

use serde::de::DeserializeOwned;

use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpCommandError {
    InvalidArguments {
        command: String,
        details: String,
    },
    Http {
        command: String,
        status: u16,
        details: String,
    },
    Transport {
        command: String,
        is_timeout: bool,
        is_connect: bool,
        details: String,
    },
    InvalidResponse {
        command: String,
        details: String,
    },
}

impl HttpCommandError {
    pub(crate) fn is_track_staging_failure(&self) -> bool {
        matches!(self, Self::Http { command, status: 500, .. } if command == "player_queues/play_media")
    }
}

impl std::fmt::Display for HttpCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArguments { command, details } => write!(
                formatter,
                "Music Assistant command '{command}' has invalid arguments: {details}"
            ),
            Self::Http {
                command,
                status,
                details,
            } => write!(
                formatter,
                "Music Assistant command '{command}' returned HTTP {status}: {details}"
            ),
            Self::Transport {
                command, details, ..
            } => write!(
                formatter,
                "Music Assistant command '{command}' transport failed: {details}"
            ),
            Self::InvalidResponse { command, details } => write!(
                formatter,
                "Music Assistant command '{command}' returned invalid JSON: {details}"
            ),
        }
    }
}

impl From<HttpCommandError> for String {
    fn from(error: HttpCommandError) -> Self {
        error.to_string()
    }
}

pub(crate) struct MusicAssistantClient {
    base_url: String,
    endpoint: String,
    pub(super) access_token: Option<AccessToken>,
    pub(super) http_client: reqwest::Client,
    pub(super) current_user_id: tokio::sync::OnceCell<UserId>,
    pub(super) request_scheduler: RequestScheduler,
}

impl MusicAssistantClient {
    // Construction.
    pub(crate) fn new(base_url: &str, access_token: Option<AccessToken>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            endpoint: format!("{}/api", base_url.trim_end_matches('/')),
            access_token,
            current_user_id: tokio::sync::OnceCell::new(),
            request_scheduler: RequestScheduler::default(),
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("create Music Assistant HTTP client"),
        }
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn begin_startup(&self) -> Box<dyn Send> {
        Box::new(self.request_scheduler.begin_startup())
    }

    // Read groups expose scheduling ownership, not HTTP arguments or result storage.
    pub(crate) fn create_read_group(&self) -> Arc<super::request_scheduler::ReadGroup> {
        self.request_scheduler.create_read_group()
    }

    #[cfg(test)]
    pub(crate) fn pending_read_count(&self) -> usize {
        self.request_scheduler.pending_read_count()
    }

    // HTTP command transport.
    pub(crate) async fn execute_command<Response>(
        &self,
        command: &str,
        arguments: Value,
    ) -> Result<Response, String>
    where
        Response: DeserializeOwned,
    {
        let response: Value = self
            .execute_http_command(command, arguments)
            .await
            .map_err(String::from)?;
        serde_json::from_value(response).map_err(|error| {
            format!("Music Assistant command '{command}' returned invalid data: {error}")
        })
    }

    pub(super) async fn execute_http_command<Response>(
        &self,
        command: &str,
        arguments: Value,
    ) -> Result<Response, HttpCommandError>
    where
        Response: DeserializeOwned,
    {
        let client = self.http_client.clone();
        let endpoint = self.endpoint.clone();
        let access_token = self.access_token.clone();
        let key = command_read_key(command, &arguments);
        let owned_command = command.to_owned();
        let operation = async move {
            send_http_command(
                &client,
                &endpoint,
                access_token.as_ref(),
                &owned_command,
                arguments,
            )
            .await
        };
        let response = if is_read_command(command) {
            if command == "providers/icon" {
                with_request_priority(
                    RequestPriority::Icon,
                    self.request_scheduler.execute(key, operation),
                )
                .await
            } else {
                self.request_scheduler.execute(key, operation).await
            }
        } else {
            operation.await
        }?;
        serde_json::from_value(response).map_err(|error| HttpCommandError::InvalidResponse {
            command: command.to_owned(),
            details: error.to_string(),
        })
    }
}

// Only idempotent queries share work; playback and mutations never enter the read queue.
fn is_read_command(command: &str) -> bool {
    matches!(
        command,
        "info"
            | "auth/me"
            | "providers"
            | "providers/manifests"
            | "providers/icon"
            | "music/item"
            | "music/get_library_item"
            | "music/search"
            | "music/recently_played_items"
            | "music/recommendations"
            | "music/recommendations/items"
    ) || command.starts_with("music/")
        && matches!(
            command.rsplit('/').next(),
            Some(
                "get"
                    | "library_items"
                    | "artist_albums"
                    | "track_versions"
                    | "similar_tracks"
                    | "similar_artists"
                    | "playlist_tracks"
                    | "album_tracks"
                    | "top_tracks"
                    | "artist_tracks"
            )
        )
}

async fn send_http_command(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: Option<&AccessToken>,
    command: &str,
    arguments: Value,
) -> Result<Value, HttpCommandError> {
    let mut request = client.post(endpoint).json(&json!({
        "command": command,
        "args": arguments,
    }));
    if let Some(access_token) = access_token.filter(|token| !token.as_str().trim().is_empty()) {
        request = request.bearer_auth(access_token.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|error| HttpCommandError::Transport {
            command: command.to_string(),
            is_timeout: error.is_timeout(),
            is_connect: error.is_connect(),
            details: error.to_string(),
        })?;
    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|error| HttpCommandError::Transport {
            command: command.to_string(),
            is_timeout: error.is_timeout(),
            is_connect: error.is_connect(),
            details: error.to_string(),
        })?;
    if !status.is_success() {
        let response_excerpt = response_body.chars().take(512).collect::<String>();
        return Err(HttpCommandError::Http {
            command: command.to_string(),
            status: status.as_u16(),
            details: response_excerpt,
        });
    }
    serde_json::from_str(&response_body).map_err(|error| {
        let response_excerpt = response_body.chars().take(512).collect::<String>();
        HttpCommandError::InvalidResponse {
            command: command.to_string(),
            details: format!("{error}; response: {response_excerpt}"),
        }
    })
}

// Transport always sends modeled media objects, including nested media type tags.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::domain::identifiers::MediaId;

    use serde_json::{json, Value};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    pub(crate) fn hold_startup(client: &MusicAssistantClient) -> impl Drop {
        client.request_scheduler.begin_startup()
    }

    pub(crate) fn server(
        responses: Vec<(&'static str, Value)>,
    ) -> (MusicAssistantClient, std::thread::JoinHandle<Vec<Value>>) {
        server_with_request_handler(responses, |_| {})
    }

    pub(crate) fn server_with_request_handler(
        responses: Vec<(&'static str, Value)>,
        mut handle_request: impl FnMut(&Value) + Send + 'static,
    ) -> (MusicAssistantClient, std::thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let mut responses = responses;
            while !responses.is_empty() {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (body_start, length) = loop {
                    let mut buffer = [0; 4096];
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&buffer[..read]);
                    if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < body_start + length {
                    let mut buffer = [0; 4096];
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&buffer[..read]);
                }
                let request: Value =
                    serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
                let response = {
                    let position = responses
                        .iter()
                        .position(|(command, _)| request["command"] == *command)
                        .expect("unexpected test server command");
                    let (expected, response) = responses.remove(position);
                    assert_eq!(request["command"], expected);
                    handle_request(&request);
                    requests.push(request);
                    response
                };
                let status = response
                    .get("test_http_status")
                    .and_then(Value::as_u64)
                    .unwrap_or(200);
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            requests
        });
        (
            MusicAssistantClient::new(&format!("http://{address}"), None),
            worker,
        )
    }

    #[tokio::test]
    async fn existing_artwork_never_fetches_provider_items() {
        let (client, worker) = server(vec![(
            "music/artists/get",
            json!({"media_type":"artist", "item_id":"original", "provider":"library", "name":"Original",
            "metadata":{"images":[{"type":"thumb", "path":"https://example.com/original.jpg", "provider":"library"}]},
            "provider_mappings":[{"item_id":"other", "provider_instance":"other", "provider_domain":"other"}]}),
        )]);
        let crate::domain::catalog::CatalogValue::Artist(artist) =
            crate::data::music_assistant::catalog::load_catalog(
                &client,
                &crate::domain::catalog::CatalogQuery::Artist {
                    item_id: MediaId("original".into()),
                    provider: "library".into(),
                },
                false,
            )
            .await
            .unwrap()
        else {
            panic!("expected artist")
        };
        assert_eq!(
            artist.media.metadata.unwrap().images[0].path,
            "https://example.com/original.jpg"
        );
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn playlist_tasks_are_confirmed_and_failures_are_returned() {
        for status in ["success", "failed"] {
            let (client, worker) = server(vec![
                (
                    "music/playlists/add_playlist_tracks",
                    json!({"id":"task", "status":"pending"}),
                ),
                (
                    "tasks/get",
                    json!({"id":"task", "status":status, "last_error":"test failure"}),
                ),
            ]);
            let result = client
                .add_playlist_tracks("42", vec!["test://track/a".into()])
                .await;
            assert_eq!(result.is_ok(), status == "success");
            worker.join().unwrap();
        }
    }
}
