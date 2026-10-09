//! ListenBrainz HTTP boundary, typed submission metadata and redacted credentials.

use crate::music_assistant::models::media::Track;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const API_URL: &str = "https://api.listenbrainz.org/1";

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListenBrainzCredentials {
    pub(crate) username: String,
    pub(crate) token: String,
}

impl std::fmt::Debug for ListenBrainzCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ListenBrainzCredentials")
            .field("username", &self.username)
            .field("token", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Listen {
    pub(super) listened_at: i64,
    track_metadata: TrackMetadata,
}

#[derive(Clone, Serialize, Deserialize)]
struct TrackMetadata {
    artist_name: String,
    track_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_name: Option<String>,
    additional_info: AdditionalInfo,
}

#[derive(Clone, Serialize, Deserialize)]
struct AdditionalInfo {
    submission_client: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_mbid: Option<String>,
}

#[derive(Deserialize)]
struct TokenValidation {
    valid: bool,
    user_name: Option<String>,
}

#[derive(Serialize)]
struct ListenSubmission<'a> {
    listen_type: &'static str,
    payload: &'a [Listen],
}

pub(super) enum SubmissionOutcome {
    Accepted,
    Retry {
        status: Option<u16>,
        after: Duration,
    },
}

#[derive(Clone)]
pub(super) struct ListenBrainzClient {
    http: reqwest::Client,
    api_url: String,
}

impl ListenBrainzClient {
    // Construction and HTTP configuration.
    pub(super) fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("Mashin/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| "ListenBrainz HTTP client could not start".to_string())?;
        Ok(Self {
            http,
            api_url: API_URL.to_string(),
        })
    }

    // Credential verification and listen submission.
    async fn validate(&self, credentials: &ListenBrainzCredentials) -> Result<(), String> {
        let response = self
            .http
            .get(format!("{}/validate-token", self.api_url))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Token {}", credentials.token),
            )
            .send()
            .await
            .map_err(|_| "ListenBrainz could not be reached".to_string())?;
        let validation: TokenValidation = response
            .error_for_status()
            .map_err(|_| "ListenBrainz rejected the token".to_string())?
            .json()
            .await
            .map_err(|_| "ListenBrainz returned an invalid response".to_string())?;
        if !validation.valid {
            return Err("ListenBrainz token is invalid".to_string());
        }
        if validation.user_name.as_deref() != Some(credentials.username.as_str()) {
            return Err("ListenBrainz token does not belong to this username".to_string());
        }
        Ok(())
    }

    pub(super) async fn submit(
        &self,
        credentials: &ListenBrainzCredentials,
        listens: &[Listen],
    ) -> SubmissionOutcome {
        let result = self
            .http
            .post(format!("{}/submit-listens", self.api_url))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Token {}", credentials.token),
            )
            .json(&ListenSubmission {
                listen_type: "import",
                payload: listens,
            })
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => SubmissionOutcome::Accepted,
            Ok(response) => {
                let seconds = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(60)
                    .clamp(30, 86_400);
                SubmissionOutcome::Retry {
                    status: Some(response.status().as_u16()),
                    after: Duration::from_secs(seconds),
                }
            }
            Err(_) => SubmissionOutcome::Retry {
                status: None,
                after: Duration::from_secs(60),
            },
        }
    }
}

// Blocking platform commands validate account ownership before persisting credentials.
pub(crate) fn validate_credentials(
    mut credentials: ListenBrainzCredentials,
) -> Result<ListenBrainzCredentials, String> {
    credentials.username = credentials.username.trim().to_string();
    credentials.token = credentials.token.trim().to_string();
    if credentials.username.is_empty() && credentials.token.is_empty() {
        return Ok(credentials);
    }
    if credentials.username.is_empty() || credentials.token.is_empty() {
        return Err("ListenBrainz username and token are both required".to_string());
    }
    let client = ListenBrainzClient::new()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "ListenBrainz validation could not start".to_string())?;
    runtime.block_on(client.validate(&credentials))?;
    Ok(credentials)
}

impl Listen {
    // Track metadata mapping at the ListenBrainz boundary.
    pub(super) fn from_track(track: &Track, listened_at: i64) -> Option<Self> {
        let artist_name = track
            .artists
            .iter()
            .map(|artist| artist.media.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if artist_name.trim().is_empty() || track.media.name.trim().is_empty() {
            return None;
        }
        Some(Self {
            listened_at,
            track_metadata: TrackMetadata {
                artist_name,
                track_name: if track.media.version.is_empty() {
                    track.media.name.clone()
                } else {
                    format!("{} ({})", track.media.name, track.media.version)
                },
                release_name: track.album.as_ref().map(|album| album.media.name.clone()),
                additional_info: AdditionalInfo {
                    submission_client: "Mashin".to_string(),
                    duration_ms: u64::from(track.duration) * 1_000,
                    recording_mbid: track
                        .media
                        .external_ids
                        .iter()
                        .find(|pair| {
                            pair.first().map(String::as_str) == Some("musicbrainz_recording")
                        })
                        .and_then(|pair| pair.get(1))
                        .cloned(),
                },
            },
        })
    }
}

#[cfg(test)]
pub(super) fn test_listen() -> Listen {
    Listen::from_track(&tests::test_track(), 123).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    pub(super) fn test_track() -> Track {
        serde_json::from_value(serde_json::json!({
            "item_id": "track-one", "provider": "test", "name": "Track", "version": "Live", "duration": 180,
            "external_ids": [["musicbrainz_recording", "recording-id"]],
            "artists": [{"item_id": "artist-one", "provider": "test", "name": "Artist"}],
            "album": {"item_id": "album-one", "provider": "test", "name": "Album"}
        })).expect("parse test track")
    }

    fn mock_server(
        status: &str,
        body: &str,
        headers: &str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}", body.len());
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = socket.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            socket.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, worker)
    }

    #[test]
    fn credentials_debug_output_redacts_the_token() {
        let credentials = ListenBrainzCredentials {
            username: "user".into(),
            token: "secret".into(),
        };
        assert!(!format!("{credentials:?}").contains("secret"));
    }

    #[test]
    fn empty_credentials_disable_and_partial_credentials_are_rejected() {
        assert_eq!(
            validate_credentials(ListenBrainzCredentials::default()).unwrap(),
            ListenBrainzCredentials::default()
        );
        assert!(validate_credentials(ListenBrainzCredentials {
            username: "user".into(),
            token: String::new()
        })
        .is_err());
        assert!(validate_credentials(ListenBrainzCredentials {
            username: String::new(),
            token: "secret".into()
        })
        .is_err());
    }

    #[test]
    fn token_validation_uses_authorization_and_checks_account_ownership() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let credentials = ListenBrainzCredentials {
            username: "user".into(),
            token: "secret".into(),
        };
        for (body, should_succeed) in [
            (r#"{"valid":true,"user_name":"user"}"#, true),
            (r#"{"valid":true,"user_name":"other"}"#, false),
            (r#"{"valid":false}"#, false),
        ] {
            let (api_url, server) = mock_server("200 OK", body, "");
            let mut client = ListenBrainzClient::new().unwrap();
            client.api_url = api_url;
            assert_eq!(
                runtime.block_on(client.validate(&credentials)).is_ok(),
                should_succeed
            );
            let request = server.join().unwrap();
            assert!(request.starts_with("GET /validate-token HTTP/1.1"));
            assert!(request
                .to_lowercase()
                .contains("authorization: token secret"));
            assert!(!request.lines().next().unwrap().contains("secret"));
        }
    }

    #[test]
    fn submission_sends_import_payload_and_honors_rate_limit() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let credentials = ListenBrainzCredentials {
            username: "user".into(),
            token: "secret".into(),
        };
        let (api_url, server) = mock_server("429 Too Many Requests", "{}", "Retry-After: 120\r\n");
        let mut client = ListenBrainzClient::new().unwrap();
        client.api_url = api_url;
        let outcome = runtime.block_on(client.submit(&credentials, &[test_listen()]));
        assert!(
            matches!(outcome, SubmissionOutcome::Retry { status: Some(429), after } if after == Duration::from_secs(120))
        );
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /submit-listens HTTP/1.1"));
        let payload: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["listen_type"], "import");
        assert_eq!(payload["payload"][0]["listened_at"], 123);
        assert!(!payload.to_string().contains("secret"));
    }

    #[test]
    fn submission_payload_preserves_metadata_and_recording_id() {
        let listen = test_listen();
        let payload = serde_json::to_value(listen).unwrap();
        let metadata = &payload["track_metadata"];
        assert_eq!(metadata["artist_name"], "Artist");
        assert_eq!(metadata["track_name"], "Track (Live)");
        assert_eq!(metadata["release_name"], "Album");
        assert_eq!(
            metadata["additional_info"]["recording_mbid"],
            "recording-id"
        );
        assert_eq!(metadata["additional_info"]["duration_ms"], 180_000);
        assert_eq!(metadata["additional_info"]["submission_client"], "Mashin");
    }

    #[test]
    fn listens_require_artist_and_title_but_not_musicbrainz_ids() {
        let mut track = test_track();
        track.media.external_ids.clear();
        assert!(Listen::from_track(&track, 123).is_some());
        track.artists.clear();
        assert!(Listen::from_track(&track, 123).is_none());
        let mut track = test_track();
        track.media.name.clear();
        assert!(Listen::from_track(&track, 123).is_none());
    }
}
