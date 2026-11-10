//! Proxy byte transport uses the session's shared read scheduler and HTTP client.
//! Bytes remain untrusted until the image worker validates them; 404 alone is absence.

use std::sync::Arc;

use super::client::{HttpCommandError, MusicAssistantClient};
use crate::data::cache::image_sources::{ImageSourceKey, ImageVariantKey};

pub(in crate::data) enum ImageFetch {
    Found(Arc<[u8]>),
    NotFound,
}

impl MusicAssistantClient {
    // Image reads share priority, in-flight deduplication and subscriber cancellation with JSON.
    pub(in crate::data) async fn fetch_proxy_image(
        &self,
        key: &ImageVariantKey,
        generation_id: uuid::Uuid,
    ) -> Result<ImageFetch, HttpCommandError> {
        let url = proxy_url(self.base_url(), key)?;
        let mut request = self.http_client.get(url.clone());
        if let Some(token) = self.access_token.as_ref() {
            request = request.bearer_auth(token.as_str());
        }
        let result = self
            .request_scheduler
            .execute_bytes(format!("{generation_id}:{url}"), async move {
                let response = request.send().await.map_err(transport_error)?;
                let status = response.status();
                if !status.is_success() {
                    return Err(HttpCommandError::Http {
                        command: "imageproxy".into(),
                        status: status.as_u16(),
                        details: "image proxy returned an unsuccessful status".into(),
                    });
                }
                response
                    .bytes()
                    .await
                    .map(|bytes| bytes.to_vec())
                    .map_err(transport_error)
            })
            .await;
        match result {
            Ok(bytes) => Ok(ImageFetch::Found(bytes)),
            Err(HttpCommandError::Http { status: 404, .. }) => Ok(ImageFetch::NotFound),
            Err(error) => Err(error),
        }
    }
}

// Transport parameters never replace the exact observed source identity in the image index.
fn proxy_url(base_url: &str, key: &ImageVariantKey) -> Result<reqwest::Url, HttpCommandError> {
    let invalid = || HttpCommandError::InvalidArguments {
        command: "imageproxy".into(),
        details: "invalid server address or unsupported image source".into(),
    };
    let ImageSourceKey::ProxyId(proxy_id) = &key.source else {
        return Err(invalid());
    };
    if proxy_id.len() != 64 || !proxy_id.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut url = reqwest::Url::parse(base_url).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|_| invalid())?
        .pop_if_empty()
        .push("imageproxy")
        .push(proxy_id);
    url.query_pairs_mut()
        .append_pair("size", &key.parameters.size.pixels().to_string());
    if let Some(format) = key.parameters.output_format.proxy_parameter() {
        url.query_pairs_mut().append_pair("fmt", format);
    }
    Ok(url)
}

fn transport_error(error: reqwest::Error) -> HttpCommandError {
    HttpCommandError::Transport {
        command: "imageproxy".into(),
        is_timeout: error.is_timeout(),
        is_connect: error.is_connect(),
        details: "image proxy transport failed".into(),
    }
}

#[cfg(test)]
pub(in crate::data) mod tests {
    use super::*;
    use crate::data::cache::image_sources::{ImageOutputFormat, ImageSize, ImageVariantParameters};
    use crate::domain::identifiers::ProviderInstanceId;
    use crate::domain::session::AccessToken;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn key() -> ImageVariantKey {
        ImageVariantKey {
            source: ImageSourceKey::ProxyId("Ab".repeat(32)),
            parameters: ImageVariantParameters {
                size: ImageSize::Pixels256,
                output_format: ImageOutputFormat::JpegPreserveTransparency,
            },
        }
    }

    fn server(
        status: u16,
        bytes: Vec<u8>,
    ) -> (MusicAssistantClient, std::thread::JoinHandle<String>) {
        server_with_request_handler(status, bytes, || {})
    }

    pub(in crate::data) fn server_with_request_handler(
        status: u16,
        bytes: Vec<u8>,
        handle_request: impl FnOnce() + Send + 'static,
    ) -> (MusicAssistantClient, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|value| value == b"\r\n\r\n") {
                    break;
                }
            }
            handle_request();
            let result = write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len())
                .and_then(|_| stream.write_all(&bytes));
            if let Err(error) = result {
                assert!(
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                    ),
                    "unexpected test server write failure: {error}"
                );
            }
            String::from_utf8(request).unwrap()
        });
        let client = MusicAssistantClient::new(
            &format!("http://{address}/prefix/?old=discard#fragment"),
            Some(AccessToken("synthetic-token".into())),
        );
        (client, worker)
    }

    #[test]
    fn proxy_url_preserves_observed_id_and_distinguishes_all_variant_parameters() {
        use ImageOutputFormat::*;
        for (format, expected) in [
            (Automatic, None),
            (JpegPreserveTransparency, Some("jpg")),
            (JpegFlattenTransparency, Some("jpeg")),
            (Png, Some("png")),
            (Svg, Some("svg")),
        ] {
            let mut key = key();
            key.parameters.output_format = format;
            let url =
                proxy_url("https://example.invalid/prefix/?old=discard#fragment", &key).unwrap();
            assert_eq!(
                url.path(),
                format!("/prefix/imageproxy/{}", "Ab".repeat(32))
            );
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query.get("size").map(|value| value.as_ref()), Some("256"));
            assert_eq!(query.get("fmt").map(|value| value.as_ref()), expected);
            assert!(!query.contains_key("old"));
            assert!(url.fragment().is_none());
        }
        let mut key = key();
        key.source = ImageSourceKey::ModelReference {
            provider: ProviderInstanceId("provider".into()),
            path: "https://outside.invalid/private".into(),
        };
        assert!(proxy_url("https://example.invalid", &key).is_err());
        key.source = ImageSourceKey::ProxyId("../invalid".into());
        assert!(proxy_url("https://example.invalid", &key).is_err());
        assert!(proxy_url("https://secret@example.invalid", &self::key()).is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_proxy_reads_share_bytes_and_send_only_canonical_parameters() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let bytes = encoded.into_inner();
        let (client, worker) = server(200, bytes.clone());
        let key = key();
        let generation_id = uuid::Uuid::new_v4();
        let (first, second) = tokio::join!(
            client.fetch_proxy_image(&key, generation_id),
            client.fetch_proxy_image(&key, generation_id)
        );
        let (ImageFetch::Found(first), ImageFetch::Found(second)) =
            (first.unwrap(), second.unwrap())
        else {
            panic!("expected image bytes")
        };
        assert_eq!(&*first, bytes);
        assert!(Arc::ptr_eq(&first, &second));
        let request = worker.join().unwrap();
        assert!(request.starts_with(&format!(
            "GET /prefix/imageproxy/{}?size=256&fmt=jpg HTTP/1.1\r\n",
            "Ab".repeat(32)
        )));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-token\r\n"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn only_not_found_is_absence_and_transport_errors_do_not_expose_urls() {
        let (client, worker) = server(404, Vec::new());
        assert!(matches!(
            client
                .fetch_proxy_image(&key(), uuid::Uuid::new_v4())
                .await
                .unwrap(),
            ImageFetch::NotFound
        ));
        worker.join().unwrap();
        let (client, worker) = server(503, Vec::new());
        assert!(matches!(
            client.fetch_proxy_image(&key(), uuid::Uuid::new_v4()).await,
            Err(HttpCommandError::Http { status: 503, .. })
        ));
        worker.join().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = MusicAssistantClient::new(&format!("http://{address}/private-secret"), None);
        let Err(error) = client.fetch_proxy_image(&key(), uuid::Uuid::new_v4()).await else {
            panic!("expected transport error")
        };
        assert!(matches!(error, HttpCommandError::Transport { .. }));
        assert!(!error.to_string().contains("private-secret"));
    }
}
