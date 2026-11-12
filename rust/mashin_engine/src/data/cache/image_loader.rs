//! Session-bound model image loading over the shared file/index owner.
//! Local bytes precede optional refresh; its subscriber owns cancellation.
//! Inline model slots persist no payload in SQL. Storage failures retain the online path.

use futures_util::future::BoxFuture;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

use super::catalog::{ServerIdentity, SessionCacheOwner};
use super::image_files::ImagePayload;
use super::image_reads::ImageRefresh;
use super::image_sources::{
    ImageOutputFormat, ImageSize, ImageSourceKey, ImageVariantKey, ImageVariantParameters,
};
use super::lifecycle::ImageLease;
use crate::application::repositories::{ImageLoad, ImageLoader, ImageUpdate};
use crate::data::music_assistant::client::MusicAssistantClient;
use crate::data::music_assistant::images::ImageFetch;
use crate::domain::artwork::{is_inline_image, ImageRequest};

#[derive(Clone)]
pub(crate) struct SessionImages {
    inner: Arc<ImageSession>,
}

struct ImageSession {
    owner: Arc<SessionCacheOwner>,
    client: Arc<MusicAssistantClient>,
    generation: u64,
    transport_generation: uuid::Uuid,
    bound: tokio::sync::Mutex<Option<Arc<BoundImages>>>,
}

struct BoundImages {
    images: ImageLease,
    server_id: String,
    verified: OnceCell<()>,
}

impl SessionImages {
    // The session factory supplies the same client and owner used by the catalog.
    pub(crate) fn new(
        owner: Arc<SessionCacheOwner>,
        client: Arc<MusicAssistantClient>,
        generation: u64,
    ) -> Self {
        Self {
            inner: Arc::new(ImageSession {
                owner,
                client,
                generation,
                transport_generation: uuid::Uuid::new_v4(),
                bound: tokio::sync::Mutex::new(None),
            }),
        }
    }

    // All reads fence the session before and after independent file/CPU/network work.
    async fn read(&self, request: &ImageRequest) -> Result<ImageLoad, String> {
        self.require_active()?;
        let size = request.size;
        ImageSize::from_pixels(size).map_err(|error| error.to_string())?;
        let is_inline = is_inline_image(&request.image);
        let cached_inline = request.image.cached_inline.as_ref();
        if cached_inline.is_some()
            && (!request.image.path.is_empty() || request.provider_icon_role.is_some())
        {
            return Err("cached inline image requires a payload-free media reference".into());
        }
        if request.provider_icon_role.as_deref() == Some("default")
            && request.image.path.is_empty()
            && request.image.proxy_id.is_none()
            && request.owner_uri.is_none()
        {
            let key = provider_icon_key(
                request.image.provider.as_str(),
                super::providers::ProviderIconRole::Default,
            )?;
            let (bound, cache_error) = match self.resolve().await {
                Ok(bound) => (Some(bound), None),
                Err(error) => (None, Some(error)),
            };
            return self
                .read_default_icon(request.image.provider.as_str(), key, bound, cache_error)
                .await;
        }
        if request.provider_icon_role.is_some() && (!is_inline || request.owner_uri.is_some()) {
            return Err("provider icon requires inline content without a media owner".into());
        }
        let source = if let Some(role) = &request.provider_icon_role {
            ImageSourceKey::from_provider_icon(&request.image, role)
        } else if let Some(reference) = cached_inline {
            ImageSourceKey::from_inline(
                &request.image,
                reference
                    .owner_uri
                    .as_ref()
                    .map(|uri| uri.as_str())
                    .unwrap_or(""),
                reference.position,
            )
        } else if is_inline {
            ImageSourceKey::from_inline(
                &request.image,
                request
                    .owner_uri
                    .as_ref()
                    .map(|uri| uri.as_str())
                    .unwrap_or(""),
                request.position,
            )
        } else {
            ImageSourceKey::from_image(&request.image)
        };
        let bound = match self.resolve().await {
            Ok(bound) => bound,
            Err(error) => return self.read_without_cache(request, source.ok(), error).await,
        };
        if is_inline {
            let payload = bound
                .images
                .validate_inline(request.image.path.clone())
                .await
                .map_err(|error| error.to_string())?;
            if let Ok(source) = &source {
                if bound.verified.get().is_none() {
                    let session = self.clone();
                    let source = source.clone();
                    let bytes = payload.bytes.clone();
                    let mut invalidation = bound.images.observe_invalidation();
                    self.require_active()?;
                    return Ok(ImageLoad {
                        current: ImageUpdate {
                            bytes: Some(payload.bytes),
                            cache_error: None,
                        },
                        refresh: Some(Box::pin(async move {
                            let operation = async {
                                session.verify_server(&bound).await?;
                                let cache_error = bound
                                    .images
                                    .publish_variant(
                                        key_for_size(source, size)?,
                                        bytes.clone(),
                                        chrono::Utc::now().timestamp(),
                                    )
                                    .await
                                    .err()
                                    .map(|error| error.to_string());
                                session.require_active()?;
                                Ok(ImageUpdate {
                                    bytes: Some(bytes),
                                    cache_error,
                                })
                            };
                            tokio::select! {
                                biased;
                                _ = async { while !*invalidation.borrow() { if invalidation.changed().await.is_err() { break; } } } => Err("image cache generation invalidated".into()),
                                result = operation => result,
                            }
                        })),
                    });
                }
            }
            let cache_error = match source {
                Ok(source) => bound
                    .images
                    .publish_variant(
                        key_for_size(source, size)?,
                        payload.bytes.clone(),
                        chrono::Utc::now().timestamp(),
                    )
                    .await
                    .err()
                    .map(|error| error.to_string()),
                Err(error) => Some(error.to_string()),
            };
            self.require_active()?;
            return Ok(ImageLoad {
                current: ImageUpdate {
                    bytes: Some(payload.bytes),
                    cache_error,
                },
                refresh: None,
            });
        }
        let source = source.map_err(|error| error.to_string())?;
        let can_fetch_proxy = matches!(source, ImageSourceKey::ProxyId(_));
        if !can_fetch_proxy && cached_inline.is_none() {
            return Err("image has no supported proxy or inline reference".into());
        }
        let mut read_key = key_for_size(source.clone(), size)?;
        let mut thumbnail_error = None;
        let state = if cached_inline.is_some() {
            match bound.images.read_inline_refresh_state(read_key.clone()).await {
                Ok(state) => Ok(state),
                Err(super::database::CacheError::Invalidated
                    | super::database::CacheError::ShuttingDown) => {
                    return Err("image cache generation invalidated".into());
                }
                Err(error) => {
                    thumbnail_error = Some(error);
                    read_key = key_for_size(source, 0)?;
                    bound.images.read_refresh_state(read_key.clone()).await
                }
            }
        } else {
            bound.images.read_refresh_state(read_key.clone()).await
        };
        let mut read = bound.images.begin_read_with_state(
            read_key,
            can_fetch_proxy.then(|| self.inner.client.clone()),
            Duration::from_secs(300),
            false,
            state,
        ).map_err(|error| error.to_string())?;
        let session = self.clone();
        let refresh = read.refresh.take().map(|refresh| {
            let mut invalidation = bound.images.observe_invalidation();
            Box::pin(async move {
            let operation = async {
            session.verify_server(&bound).await?;
            let update = match refresh.await.map_err(|error| format!("image refresh failed: {error:?}"))? {
                ImageRefresh::Found { payload, cache_error } => ImageUpdate { bytes: Some(payload.bytes), cache_error: cache_error.map(|error| error.to_string()) },
                ImageRefresh::NotFound { cache_error } => ImageUpdate { bytes: None, cache_error: cache_error.map(|error| error.to_string()) },
            };
            session.require_active()?;
            Ok(update)
            };
            tokio::select! {
                biased;
                _ = async { while !*invalidation.borrow() { if invalidation.changed().await.is_err() { break; } } } => Err("image cache generation invalidated".into()),
                result = operation => result,
            }
        }) as BoxFuture<'static, Result<ImageUpdate, String>> });
        self.require_active()?;
        Ok(ImageLoad {
            current: ImageUpdate {
                bytes: read.cached.map(|cached| cached.read.payload.bytes),
                cache_error: read
                    .cache_error
                    .or(thumbnail_error)
                    .map(|error| error.to_string()),
            },
            refresh,
        })
    }

    // Default icons retain their shared size-zero source; network work belongs to the optional follower.
    async fn read_default_icon(
        &self,
        domain: &str,
        key: ImageVariantKey,
        bound: Option<Arc<BoundImages>>,
        mut cache_error: Option<String>,
    ) -> Result<ImageLoad, String> {
        let mut bytes = None;
        let mut is_recently_not_found = false;
        let mut is_fresh = false;
        if let Some(bound) = &bound {
            match bound.images.read_refresh_state(key).await {
                Ok(state) => {
                    is_fresh = state.is_fresh_for(Duration::from_secs(300));
                    is_recently_not_found = state.is_recently_not_found;
                    if !is_recently_not_found {
                        bytes = state.cached.map(|cached| cached.read.payload.bytes);
                    }
                }
                Err(error) => cache_error = Some(error.to_string()),
            }
        }
        self.require_active()?;
        let current = ImageUpdate {
            bytes,
            cache_error: cache_error.clone(),
        };
        if is_fresh || is_recently_not_found {
            return Ok(ImageLoad {
                current,
                refresh: None,
            });
        }
        let session = self.clone();
        let domain = domain.to_owned();
        let mut invalidation = bound
            .as_ref()
            .map(|bound| bound.images.observe_invalidation());
        Ok(ImageLoad {
            current,
            refresh: Some(Box::pin(async move {
                let operation = async {
                    session.require_active()?;
                    if let Some(bound) = &bound {
                        session.verify_server(bound).await?;
                    }
                    let inline = crate::application::repositories::ProviderClient::get_icon_data(
                        &*session.inner.client,
                        &domain,
                    )
                    .await?;
                    session.require_active()?;
                    let bytes = if let Some(inline) = &inline {
                        let payload = if let Some(bound) = &bound {
                            bound
                                .images
                                .validate_inline(inline.clone())
                                .await
                                .map_err(|error| error.to_string())?
                        } else {
                            let inline = inline.clone();
                            tokio::task::spawn_blocking(move || {
                                ImagePayload::decode_inline(&inline)
                            })
                            .await
                            .map_err(|error| error.to_string())?
                            .map_err(|error| error.to_string())?
                        };
                        Some(payload.bytes)
                    } else {
                        None
                    };
                    if let Some(bound) = &bound {
                        cache_error = persist_provider_icon(&bound.images, &domain, inline)
                            .await
                            .err()
                            .or(cache_error);
                    }
                    session.require_active()?;
                    Ok(ImageUpdate { bytes, cache_error })
                };
                tokio::select! {
                    biased;
                    _ = async {
                        if let Some(invalidation) = &mut invalidation {
                            while !*invalidation.borrow() { if invalidation.changed().await.is_err() { break; } }
                        } else { std::future::pending::<()>().await; }
                    } => Err("image cache generation invalidated".into()),
                    result = operation => result,
                }
            })),
        })
    }

    // Without writable cache storage, only validated inline bytes or the existing proxy are used.
    async fn read_without_cache(
        &self,
        request: &ImageRequest,
        source: Option<ImageSourceKey>,
        error: String,
    ) -> Result<ImageLoad, String> {
        self.require_active()?;
        if is_inline_image(&request.image) {
            let source = request.image.path.clone();
            let payload = tokio::task::spawn_blocking(move || ImagePayload::decode_inline(&source))
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            self.require_active()?;
            return Ok(ImageLoad {
                current: ImageUpdate {
                    bytes: Some(payload.bytes),
                    cache_error: Some(error),
                },
                refresh: None,
            });
        }
        let source = source
            .filter(|source| matches!(source, ImageSourceKey::ProxyId(_)))
            .ok_or("image has no supported proxy or inline reference")?;
        let session = self.clone();
        let cache_error = error.clone();
        let size = request.size;
        Ok(ImageLoad {
            current: ImageUpdate {
                bytes: None,
                cache_error: Some(error),
            },
            refresh: Some(Box::pin(async move {
                session.require_active()?;
                let bytes = match session
                    .inner
                    .client
                    .fetch_proxy_image(
                        &key_for_size(source, size)?,
                        session.inner.transport_generation,
                    )
                    .await
                    .map_err(|error| error.to_string())?
                {
                    ImageFetch::Found(bytes) => Some(
                        tokio::task::spawn_blocking(move || {
                            ImagePayload::decode_bytes(bytes.to_vec())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())?
                        .bytes,
                    ),
                    ImageFetch::NotFound => None,
                };
                session.require_active()?;
                Ok(ImageUpdate {
                    bytes,
                    cache_error: Some(cache_error),
                })
            })),
        })
    }

    // Resolution never acquires a media database lease.
    async fn resolve(&self) -> Result<Arc<BoundImages>, String> {
        let mut current = self.inner.bound.lock().await;
        self.require_active()?;
        if let Some(bound) = current
            .as_ref()
            .filter(|bound| bound.images.require_active().is_ok())
        {
            return Ok(bound.clone());
        }
        let (images, server_id, is_verified) = self
            .inner
            .owner
            .image_lease(&self.inner.client, self.inner.generation)
            .await?;
        let verified = OnceCell::new();
        if is_verified {
            let _ = verified.set(());
        }
        self.require_active()?;
        let bound = Arc::new(BoundImages {
            images,
            server_id,
            verified,
        });
        *current = Some(bound.clone());
        Ok(bound)
    }

    fn require_active(&self) -> Result<(), String> {
        self.inner.owner.require_generation(self.inner.generation)
    }

    // An address remembered on disk is not sufficient authority for new image publication.
    async fn verify_server(&self, bound: &BoundImages) -> Result<(), String> {
        self.require_active()?;
        bound
            .verified
            .get_or_try_init(|| async {
                let identity: ServerIdentity = self
                    .inner
                    .client
                    .execute_command("info", serde_json::json!({}))
                    .await?;
                if identity.server_id != bound.server_id {
                    return Err("image server identity changed; reopen the session".to_owned());
                }
                Ok(())
            })
            .await?;
        self.require_active()
    }
}

impl ImageLoader for SessionImages {
    fn clear_image_cache(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(self.inner.owner.clear_cache_for_generation(
            self.inner.generation,
            super::lifecycle::CacheMaintenance::ClearImages,
        ))
    }

    fn read_image<'request>(
        &'request self,
        request: &'request ImageRequest,
    ) -> BoxFuture<'request, Result<ImageLoad, String>> {
        Box::pin(self.read(request))
    }
}

// Transport parameters do not encode or replace the source identity.
fn key_for_size(source: ImageSourceKey, size: u16) -> Result<ImageVariantKey, String> {
    Ok(ImageVariantKey {
        source,
        parameters: ImageVariantParameters {
            size: ImageSize::from_pixels(size).map_err(|error| error.to_string())?,
            output_format: ImageOutputFormat::Automatic,
        },
    })
}

#[cfg(test)]
fn key(source: ImageSourceKey) -> ImageVariantKey {
    key_for_size(source, 512).unwrap()
}

// Files are committed before payload-free model references can enter the media transaction.
pub(super) async fn persist_playlist_images(
    images: &ImageLease,
    playlist: &mut crate::domain::media::Playlist,
) -> Result<(), String> {
    let owner_uri = playlist.media.uri.clone();
    let Some(metadata) = playlist.media.metadata.as_mut() else {
        return Ok(());
    };
    for (position, image) in metadata.images.iter_mut().enumerate() {
        if !is_inline_image(image) {
            continue;
        }
        let position = u32::try_from(position).map_err(|_| "image position out of range")?;
        let source = ImageSourceKey::from_inline(
            image,
            owner_uri.as_ref().map(|uri| uri.as_str()).unwrap_or(""),
            position,
        )
        .map_err(|error| error.to_string())?;
        let payload = images
            .validate_inline(image.path.clone())
            .await
            .map_err(|error| error.to_string())?;
        let content_hash = format!("{:x}", Sha256::digest(&payload.bytes));
        images
            .publish_variant(
                key_for_size(source, 0)?,
                payload.bytes,
                chrono::Utc::now().timestamp(),
            )
            .await
            .map_err(|error| error.to_string())?;
        image.path.clear();
        image.cached_inline = Some(crate::domain::media::CachedInlineImage {
            owner_uri: owner_uri.clone(),
            position,
            content_hash: Some(content_hash),
        });
    }
    Ok(())
}

// Manifest bytes use the existing independent image owner before metadata admission.
pub(super) async fn persist_manifest_icons(
    images: &ImageLease,
    manifest: &crate::domain::providers::ProviderManifest,
) -> Result<(), String> {
    let mut observation = manifest.clone();
    for role in super::providers::ProviderIconRole::INLINE {
        let value = role
            .value_mut(&mut observation)
            .map_err(|error| error.to_string())?;
        let Some(inline) = value
            .as_deref()
            .filter(|value| crate::domain::artwork::is_inline_source(value))
        else {
            continue;
        };
        let key = provider_icon_key(&manifest.domain, role)?;
        let payload = images
            .validate_inline(inline.to_owned())
            .await
            .map_err(|error| error.to_string())?;
        images
            .publish_variant(key, payload.bytes, chrono::Utc::now().timestamp())
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(super) async fn restore_manifest_icons(
    images: &ImageLease,
    mut stored: super::providers::StoredManifest,
) -> Result<crate::domain::providers::ProviderManifest, String> {
    use base64::Engine;
    for role in stored.icons {
        if let Some(image) = images
            .read_variant(provider_icon_key(&stored.manifest.domain, role)?)
            .await
            .map_err(|error| error.to_string())?
        {
            let payload = &image.read.payload;
            *role
                .value_mut(&mut stored.manifest)
                .map_err(|error| error.to_string())? = Some(format!(
                "data:{};base64,{}",
                payload.mime_type,
                base64::engine::general_purpose::STANDARD.encode(&payload.bytes)
            ));
        }
    }
    Ok(stored.manifest)
}

pub(super) fn provider_icon_key(
    domain: &str,
    role: super::providers::ProviderIconRole,
) -> Result<ImageVariantKey, String> {
    let image = crate::domain::media::MediaItemImage {
        image_type: role.as_str().into(),
        provider: crate::domain::identifiers::ProviderInstanceId(domain.into()),
        path: String::new(),
        remotely_accessible: false,
        proxy_id: None,
        cached_inline: None,
    };
    key_for_size(
        ImageSourceKey::from_provider_icon(&image, role.as_str())
            .map_err(|error| error.to_string())?,
        0,
    )
}

pub(super) async fn read_provider_icon(
    images: &ImageLease,
    domain: &str,
) -> Result<Option<String>, String> {
    use base64::Engine;
    let state = images
        .read_refresh_state(provider_icon_key(
            domain,
            super::providers::ProviderIconRole::Default,
        )?)
        .await
        .map_err(|error| error.to_string())?;
    if state.is_recently_not_found {
        return Ok(None);
    }
    Ok(state.cached.map(|cached| {
        format!(
            "data:{};base64,{}",
            cached.read.payload.mime_type,
            base64::engine::general_purpose::STANDARD.encode(&cached.read.payload.bytes)
        )
    }))
}

pub(super) async fn persist_provider_icon(
    images: &ImageLease,
    domain: &str,
    inline: Option<String>,
) -> Result<(), String> {
    let key = provider_icon_key(domain, super::providers::ProviderIconRole::Default)?;
    if let Some(inline) = inline {
        let payload = images
            .validate_inline(inline)
            .await
            .map_err(|error| error.to_string())?;
        images
            .publish_variant(key, payload.bytes, chrono::Utc::now().timestamp())
            .await
            .map_err(|error| error.to_string())?;
    } else {
        let state = images
            .read_refresh_state(key.clone())
            .await
            .map_err(|error| error.to_string())?;
        let revision = state
            .cached
            .as_ref()
            .map(|cached| cached.read.source_revision);
        images
            .record_not_found(key, revision)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::image_files::tests::bitmap;
    use super::*;
    use crate::data::music_assistant::images::tests::server_with_request_handler;
    use crate::domain::identifiers::{MediaUri, ProviderInstanceId};
    use crate::domain::media::MediaItemImage;
    use base64::Engine;

    #[tokio::test(flavor = "current_thread")]
    async fn manifest_icons_round_trip_through_files_without_metadata_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client).await;
        let bound = images.resolve().await.unwrap();
        let bytes = bitmap(7);
        let inline = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        let manifest = serde_json::from_value(
            serde_json::json!({"domain":"builtin","name":"Built in","icon_svg":inline}),
        )
        .unwrap();
        persist_manifest_icons(&bound.images, &manifest)
            .await
            .unwrap();
        let stored =
            super::super::providers::StoredManifest::from_published_manifest(manifest).unwrap();
        assert!(stored.manifest.icon_svg.is_none());
        assert!(!serde_json::to_string(&stored.manifest)
            .unwrap()
            .contains("base64"));
        let restored = restore_manifest_icons(&bound.images, stored).await.unwrap();
        let payload = ImagePayload::decode_inline(restored.icon_svg.as_deref().unwrap()).unwrap();
        assert_eq!(payload.bytes, bytes);
        drop(bound);
        drop(images);
        owner.shutdown(1).await.unwrap();
    }

    fn request(path: String, proxy_id: Option<String>) -> ImageRequest {
        ImageRequest {
            image: MediaItemImage {
                image_type: "thumb".into(),
                provider: ProviderInstanceId("builtin".into()),
                path,
                remotely_accessible: false,
                proxy_id,
                cached_inline: None,
            },
            owner_uri: Some(MediaUri("library://album/7".into())),
            position: 0,
            size: 512,
            provider_icon_role: None,
        }
    }

    async fn bound(
        directory: &std::path::Path,
        client: Arc<MusicAssistantClient>,
    ) -> (Arc<SessionCacheOwner>, SessionImages) {
        let mut address = reqwest::Url::parse(client.base_url()).unwrap();
        address.set_query(None);
        address.set_fragment(None);
        let client = Arc::new(MusicAssistantClient::new(address.as_str(), None));
        let path = directory.to_owned();
        let owner = Arc::new(SessionCacheOwner::new(move || Ok(path.clone())));
        let _catalog = owner.session(client.clone(), None, 1);
        let cache = owner.cache_for_generation(1).await.unwrap();
        cache.activate_images("server".into()).await.unwrap();
        cache
            .bind_server_address("server".into(), address.to_string())
            .await
            .unwrap();
        let images = SessionImages::new(owner.clone(), client, 1);
        let verified = OnceCell::new();
        verified.set(()).unwrap();
        *images.inner.bound.lock().await = Some(Arc::new(BoundImages {
            images: cache.images().await.unwrap(),
            server_id: "server".into(),
            verified,
        }));
        (owner, images)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_reference_publication_rejects_invalid_identity_or_bytes_without_replacing_valid_file(
    ) {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client).await;
        let payload = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bitmap(5))
        );
        let proxy_id = "a".repeat(64);
        let mut baseline: crate::domain::media::Playlist = serde_json::from_value(serde_json::json!({"provider":"library","item_id":"42","name":"Private","metadata":{"images":[{"type":"thumb","provider":"builtin","path":payload,"proxy_id":proxy_id}]}})).unwrap();
        let lease = images.resolve().await.unwrap();
        persist_playlist_images(&lease.images, &mut baseline)
            .await
            .unwrap();
        assert!(baseline.media.metadata.as_ref().unwrap().images[0]
            .cached_inline
            .as_ref()
            .unwrap()
            .owner_uri
            .is_none());
        for (proxy, path) in [
            (None, payload.clone()),
            (Some("invalid".to_owned()), payload),
            (
                Some(proxy_id.clone()),
                "data:image/png;base64,broken".to_owned(),
            ),
        ] {
            let mut playlist: crate::domain::media::Playlist = serde_json::from_value(serde_json::json!({"provider":"library","item_id":"42","name":"Private","metadata":{"images":[{"type":"thumb","provider":"builtin","path":path,"proxy_id":proxy}]}})).unwrap();
            let before = playlist.clone();
            assert!(persist_playlist_images(&lease.images, &mut playlist)
                .await
                .is_err());
            assert_eq!(playlist, before);
        }
        let persisted = lease
            .images
            .read_variant(key_for_size(ImageSourceKey::ProxyId(proxy_id), 0).unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.read.payload.bytes, bitmap(5));
        drop(persisted);
        let mut incompatible = request("unexpected-payload".into(), None);
        incompatible.image.cached_inline = baseline
            .media
            .metadata
            .unwrap()
            .images
            .remove(0)
            .cached_inline;
        assert!(images.read_image(&incompatible).await.is_err());
        owner.shutdown(1).await.unwrap();
        drop(lease);
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_content_changes_update_reference_without_changing_source_identity() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client).await;
        let lease = images.resolve().await.unwrap();
        for proxy_id in [None, Some("a".repeat(64))] {
            let mut references = Vec::new();
            for value in [3, 3, 7] {
                let mut playlist: crate::domain::media::Playlist = serde_json::from_value(serde_json::json!({"provider":"library","item_id":"42","uri":"library://playlist/42","name":"Private","metadata":{"images":[{"type":"thumb","provider":"builtin","path":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(bitmap(value))),"proxy_id":proxy_id}]}})).unwrap();
                persist_playlist_images(&lease.images, &mut playlist)
                    .await
                    .unwrap();
                let image = playlist.media.metadata.unwrap().images.remove(0);
                let reference = image.cached_inline.as_ref().unwrap();
                assert_eq!(
                    reference.content_hash.as_deref(),
                    Some(format!("{:x}", Sha256::digest(bitmap(value))).as_str())
                );
                let source =
                    ImageSourceKey::from_inline(&image, "library://playlist/42", 0).unwrap();
                references.push((source, reference.clone()));
            }
            assert_eq!(references[0], references[1]);
            assert_eq!(references[0].0, references[2].0);
            assert_eq!(references[0].1.owner_uri, references[2].1.owner_uri);
            assert_eq!(references[0].1.position, references[2].1.position);
            assert_ne!(references[0].1.content_hash, references[2].1.content_hash);
        }
        owner.shutdown(1).await.unwrap();
        drop(lease);
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cached_inline_thumbnails_follow_original_revision_and_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client.clone()).await;
        let lease = images.resolve().await.unwrap();
        let mut read = request(String::new(), Some("a".repeat(64)));
        read.size = 80;
        for color in [3, 7] {
            let bitmap = image::RgbaImage::from_pixel(640, 320, image::Rgba([color, 0, 0, 255]));
            let mut encoded = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(bitmap)
                .write_to(&mut encoded, image::ImageFormat::Png)
                .unwrap();
            let mut playlist: crate::domain::media::Playlist = serde_json::from_value(serde_json::json!({
                "provider":"library", "item_id":"42", "uri":"library://playlist/42", "name":"Private",
                "metadata":{"images":[{"type":"thumb", "provider":"builtin", "proxy_id":"a".repeat(64),
                    "path":format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(encoded.into_inner()))}]}
            })).unwrap();
            persist_playlist_images(&lease.images, &mut playlist)
                .await
                .unwrap();
            read.image = playlist.media.metadata.unwrap().images.remove(0);
            let loaded = images.read_image(&read).await.unwrap();
            assert!(loaded.refresh.is_none());
            assert!(loaded.current.cache_error.is_none());
            let bytes = loaded.current.bytes.unwrap();
            let thumbnail = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert_eq!(thumbnail.dimensions(), (80, 40));
            assert_eq!(thumbnail.get_pixel(0, 0).0, [color, 0, 0, 255]);
            assert_eq!(
                images
                    .read_image(&read)
                    .await
                    .unwrap()
                    .current
                    .bytes
                    .unwrap(),
                bytes
            );
        }
        owner.shutdown(1).await.unwrap();
        drop(lease);
        drop(images);
        drop(owner);
        let (owner, images) = bound(directory.path(), client).await;
        let loaded = images.read_image(&read).await.unwrap();
        let thumbnail = image::load_from_memory(&loaded.current.bytes.unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(thumbnail.dimensions(), (80, 40));
        assert_eq!(thumbnail.get_pixel(0, 0).0, [7, 0, 0, 255]);
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn persisted_inline_references_read_files_after_restart_without_payload_or_media_database(
    ) {
        for proxy_id in [None, Some("a".repeat(64))] {
            let directory = tempfile::tempdir().unwrap();
            let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
            let (owner, images) = bound(directory.path(), client.clone()).await;
            let mut playlist: crate::domain::media::Playlist = serde_json::from_value(serde_json::json!({"provider":"library","item_id":"42","uri":"library://playlist/42","name":"Private","metadata":{"images":[{"type":"thumb","provider":"builtin","path":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(bitmap(3))),"proxy_id":proxy_id}]}})).unwrap();
            let lease = images.resolve().await.unwrap();
            persist_playlist_images(&lease.images, &mut playlist)
                .await
                .unwrap();
            let serialized = serde_json::to_string(&playlist).unwrap();
            assert!(!serialized.contains("data:image"));
            assert!(!serialized.contains("base64"));
            let restored: crate::domain::media::Playlist =
                serde_json::from_str(&serialized).unwrap();
            let image = restored.media.metadata.unwrap().images.remove(0);
            assert!(image.path.is_empty());
            assert_eq!(image.proxy_id, proxy_id);
            assert_eq!(
                image
                    .cached_inline
                    .as_ref()
                    .unwrap()
                    .owner_uri
                    .as_ref()
                    .unwrap()
                    .as_str(),
                "library://playlist/42"
            );
            owner.shutdown(1).await.unwrap();
            drop(lease);
            drop(images);
            drop(owner);
            let (owner, images) = bound(directory.path(), client).await;
            let mut read = request(String::new(), None);
            read.image = image;
            read.size = 80;
            let loaded = images.read_image(&read).await.unwrap();
            assert_eq!(loaded.current.bytes.unwrap(), bitmap(3));
            assert!(loaded.current.cache_error.is_none());
            assert!(loaded.refresh.is_none());
            owner
                .cache_for_generation(1)
                .await
                .unwrap()
                .clear_image_cache()
                .await
                .unwrap();
            let missing = images.read_image(&read).await.unwrap();
            assert!(missing.current.bytes.is_none());
            assert_eq!(missing.refresh.is_some(), proxy_id.is_some());
            owner.shutdown(1).await.unwrap();
            drop(images);
            drop(owner);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_model_images_share_storage_survive_restart_and_do_not_open_media_database() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client.clone()).await;
        let request = request(
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bitmap(1))
            ),
            None,
        );
        let load = images.read_image(&request).await.unwrap();
        assert_eq!(load.current.bytes.unwrap(), bitmap(1));
        assert!(load.current.cache_error.is_none());
        assert!(load.refresh.is_none());
        let source = ImageSourceKey::from_inline(
            &request.image,
            request.owner_uri.as_ref().unwrap().as_str(),
            0,
        )
        .unwrap();
        let lease = images.resolve().await.unwrap();
        assert_eq!(
            lease
                .images
                .read_variant(key(source.clone()))
                .await
                .unwrap()
                .unwrap()
                .read
                .payload
                .bytes,
            bitmap(1)
        );
        let mut other = request.clone();
        other.position = 1;
        let different = ImageSourceKey::from_inline(
            &other.image,
            other.owner_uri.as_ref().unwrap().as_str(),
            1,
        )
        .unwrap();
        assert!(lease
            .images
            .read_variant(key(different))
            .await
            .unwrap()
            .is_none());
        let storage = super::super::storage::CacheStorage::open(directory.path()).unwrap();
        assert!(!storage
            .database_directory()
            .join(super::super::database::DATABASE_FILE_NAME)
            .exists());
        drop(storage);
        owner.shutdown(1).await.unwrap();
        assert!(images.read_image(&request).await.is_err());
        drop(lease);
        drop(images);
        drop(owner);
        let (owner, images) = bound(directory.path(), client).await;
        assert_eq!(
            images
                .resolve()
                .await
                .unwrap()
                .images
                .read_variant(key(source))
                .await
                .unwrap()
                .unwrap()
                .read
                .payload
                .bytes,
            bitmap(1)
        );
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn proxy_refresh_populates_the_same_cache_and_retires_with_the_session() {
        let directory = tempfile::tempdir().unwrap();
        let (client, worker) = server_with_request_handler(200, bitmap(2), || {});
        let (owner, images) = bound(directory.path(), Arc::new(client)).await;
        let request = request("server-local/path".into(), Some("a".repeat(64)));
        let mut read = images.read_image(&request).await.unwrap();
        assert!(read.current.bytes.is_none());
        let update = read.refresh.take().unwrap().await.unwrap();
        assert_eq!(update.bytes.unwrap(), bitmap(2));
        assert!(update.cache_error.is_none());
        worker.join().unwrap();
        let read = images.read_image(&request).await.unwrap();
        assert_eq!(read.current.bytes.unwrap(), bitmap(2));
        assert!(read.refresh.is_none());
        owner.shutdown(1).await.unwrap();
        assert!(images.read_image(&request).await.is_err());
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_clear_cancels_identity_verification_and_rejects_late_proxy_work() {
        let directory = tempfile::tempdir().unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let (client, worker) =
            server_with_request_handler(200, br#"{"server_id":"server"}"#.to_vec(), move || {
                started.send(()).unwrap();
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
            });
        let (owner, images) = bound(directory.path(), Arc::new(client)).await;
        let old = images.resolve().await.unwrap();
        let unverified = SessionImages::new(owner.clone(), images.inner.client.clone(), 1);
        *unverified.inner.bound.lock().await = Some(Arc::new(BoundImages {
            images: old.images.clone(),
            server_id: "server".into(),
            verified: OnceCell::new(),
        }));
        let mut read = unverified
            .read_image(&request("source".into(), Some("a".repeat(64))))
            .await
            .unwrap();
        let refresh = tokio::spawn(read.refresh.take().unwrap());
        start.await.unwrap();
        owner
            .cache_for_generation(1)
            .await
            .unwrap()
            .clear_image_cache()
            .await
            .unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), refresh)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert_eq!(unverified.inner.client.pending_read_count(), 0);
        release.send(()).unwrap();
        worker.join().unwrap();
        let inline = request(
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bitmap(4))
            ),
            None,
        );
        let reopened = unverified.read_image(&inline).await.unwrap();
        assert_eq!(reopened.current.bytes.unwrap(), bitmap(4));
        assert!(reopened.current.cache_error.is_none());
        assert!(old.images.require_active().is_err());
        owner.shutdown(1).await.unwrap();
        drop(read);
        drop(unverified);
        drop(old);
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unverified_inline_bytes_precede_identity_and_a_changed_server_cannot_publish_them() {
        let directory = tempfile::tempdir().unwrap();
        let (client, worker) =
            server_with_request_handler(200, br#"{"server_id":"different"}"#.to_vec(), || {});
        let (owner, images) = bound(directory.path(), Arc::new(client)).await;
        let old = images.resolve().await.unwrap();
        *images.inner.bound.lock().await = Some(Arc::new(BoundImages {
            images: old.images.clone(),
            server_id: "server".into(),
            verified: OnceCell::new(),
        }));
        let request = request(
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bitmap(5))
            ),
            None,
        );
        let mut read = images.read_image(&request).await.unwrap();
        assert_eq!(read.current.bytes.unwrap(), bitmap(5));
        assert_eq!(images.inner.client.pending_read_count(), 0);
        let source = ImageSourceKey::from_inline(
            &request.image,
            request.owner_uri.as_ref().unwrap().as_str(),
            0,
        )
        .unwrap();
        assert!(old
            .images
            .read_variant(key(source.clone()))
            .await
            .unwrap()
            .is_none());
        assert!(read.refresh.take().unwrap().await.is_err());
        worker.join().unwrap();
        assert!(old
            .images
            .read_variant(key(source))
            .await
            .unwrap()
            .is_none());
        owner.shutdown(1).await.unwrap();
        drop(old);
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn raw_svg_and_data_uri_share_the_model_slot_and_reject_active_replacements() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client).await;
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="red"/></svg>"#;
        let mut request = request(svg.into(), None);
        let raw = images
            .read_image(&request)
            .await
            .unwrap()
            .current
            .bytes
            .unwrap();
        request.image.path = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        );
        assert_eq!(
            images
                .read_image(&request)
                .await
                .unwrap()
                .current
                .bytes
                .unwrap(),
            raw
        );
        request.image.path =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><script/></svg>"#
                .into();
        assert!(images.read_image(&request).await.is_err());
        let source = ImageSourceKey::from_inline(
            &request.image,
            request.owner_uri.as_ref().unwrap().as_str(),
            0,
        )
        .unwrap();
        assert_eq!(
            images
                .resolve()
                .await
                .unwrap()
                .images
                .read_variant(key(source))
                .await
                .unwrap()
                .unwrap()
                .read
                .payload
                .bytes,
            raw
        );
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_query_preserves_variant_size_and_provider_icons_reopen_in_the_same_index() {
        use crate::application::queries::ImageReadSnapshot;
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let (owner, images) = bound(directory.path(), client.clone()).await;
        let mut request = request(
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bitmap(6))
            ),
            None,
        );
        request.owner_uri = None;
        request.provider_icon_role = Some("icon_svg".into());
        request.size = 80;
        let wire = serde_json::to_value(&request).unwrap();
        let parsed: ImageRequest = serde_json::from_value(wire).unwrap();
        let response = ImageReadSnapshot::load(&images, &parsed, false)
            .await
            .unwrap();
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(
            json["bytes"],
            base64::engine::general_purpose::STANDARD.encode(bitmap(6))
        );
        assert_eq!(json["hasRefresh"], false);
        let source = ImageSourceKey::from_provider_icon(&request.image, "icon_svg").unwrap();
        assert!(images
            .resolve()
            .await
            .unwrap()
            .images
            .read_variant(key_for_size(source.clone(), 160).unwrap())
            .await
            .unwrap()
            .is_none());
        let mut invalid = request.clone();
        invalid.size = 81;
        assert!(images.read_image(&invalid).await.is_err());
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        let (owner, images) = bound(directory.path(), client).await;
        assert_eq!(
            images
                .resolve()
                .await
                .unwrap()
                .images
                .read_variant(key_for_size(source, 80).unwrap())
                .await
                .unwrap()
                .unwrap()
                .read
                .payload
                .bytes,
            bitmap(6)
        );
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fresh_default_icons_skip_refresh_before_and_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let mut request = request(String::new(), None);
        request.image.image_type = "default".into();
        request.owner_uri = None;
        request.provider_icon_role = Some("default".into());
        request.size = 80;
        for is_reopened in [false, true] {
            let (owner, images) = bound(directory.path(), client.clone()).await;
            if !is_reopened {
                let lease = images.resolve().await.unwrap();
                persist_provider_icon(&lease.images, "builtin", Some(format!(
                    "data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bitmap(8))
                ))).await.unwrap();
            }
            for _ in 0..2 {
                let read = images.read_image(&request).await.unwrap();
                assert_eq!(read.current.bytes, Some(bitmap(8)));
                assert!(read.refresh.is_none());
                assert_eq!(images.inner.client.pending_read_count(), 0);
            }
            owner.shutdown(1).await.unwrap();
            drop(images);
            drop(owner);
        }
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_icon_reference_reads_local_bytes_before_online_and_rejects_retired_refresh() {
        for has_icon in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
            let (owner, images) = bound(directory.path(), client).await;
            let lease = images.resolve().await.unwrap();
            let bytes = bitmap(8);
            if has_icon {
                lease.images.publish_variant(
                    provider_icon_key("builtin", super::super::providers::ProviderIconRole::Default).unwrap(),
                    bytes.clone(), 1,
                )
                .await
                .unwrap();
            }
            let mut request = request(String::new(), None);
            request.image.image_type = "default".into();
            request.owner_uri = None;
            request.provider_icon_role = Some("default".into());
            request.size = 80;
            let read = images.read_image(&request).await.unwrap();
            assert_eq!(read.current.bytes, has_icon.then_some(bytes));
            assert_eq!(images.inner.client.pending_read_count(), 0);
            assert!(read.refresh.is_some());
            images.clear_image_cache().await.unwrap();
            assert!(read.refresh.unwrap().await.is_err());
            assert_eq!(images.inner.client.pending_read_count(), 0);
            owner.shutdown(1).await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_icon_reference_followup_reuses_provider_endpoint_and_preserves_cache_on_error()
    {
        use serde_json::json;
        let cached = bitmap(7);
        let replacement = bitmap(8);
        let inline = |bytes: &[u8]| {
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        };
        for response in [
            json!(inline(&replacement)),
            json!(null),
            json!("data:image/png;base64,invalid"),
            json!({"test_http_status":503}),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (client, worker) = crate::data::music_assistant::client::tests::server(vec![(
                "providers/icon",
                response.clone(),
            )]);
            let (owner, images) = bound(directory.path(), Arc::new(client)).await;
            let lease = images.resolve().await.unwrap();
            lease.images.publish_variant(
                provider_icon_key("builtin", super::super::providers::ProviderIconRole::Default).unwrap(),
                cached.clone(), 1,
            )
                .await
                .unwrap();
            let mut request = request(String::new(), None);
            request.image.image_type = "default".into();
            request.owner_uri = None;
            request.provider_icon_role = Some("default".into());
            request.size = 80;
            let read = images.read_image(&request).await.unwrap();
            assert_eq!(read.current.bytes, Some(cached.clone()));
            assert_eq!(images.inner.client.pending_read_count(), 0);
            let refreshed = read.refresh.unwrap().await;
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), 1);
            if response.is_null() {
                assert!(refreshed.unwrap().bytes.is_none());
                let next = images.read_image(&request).await.unwrap();
                assert!(next.current.bytes.is_none());
                assert!(next.refresh.is_none());
            } else if response == json!(inline(&replacement)) {
                assert_eq!(refreshed.unwrap().bytes, Some(replacement.clone()));
                assert_eq!(
                    images.read_image(&request).await.unwrap().current.bytes,
                    Some(replacement.clone())
                );
            } else {
                assert!(refreshed.is_err());
                assert_eq!(
                    images.read_image(&request).await.unwrap().current.bytes,
                    Some(cached.clone())
                );
            }
            owner.shutdown(1).await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_query_local_miss_starts_proxy_only_in_the_scoped_follow_up() {
        use crate::adapters::models::NativeOperationRequest;
        use crate::application::queries::ImageReadSnapshot;
        let directory = tempfile::tempdir().unwrap();
        let (client, worker) = server_with_request_handler(200, bitmap(7), || {});
        let (owner, images) = bound(directory.path(), Arc::new(client)).await;
        let mut request = request("server-local/path".into(), Some("b".repeat(64)));
        request.size = 160;
        let mut envelope = serde_json::json!({"requestId":91,"kind":"readImage","request":request,"refresh":false});
        assert!(serde_json::from_value::<NativeOperationRequest>(envelope.clone()).is_ok());
        envelope["request"]["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<NativeOperationRequest>(envelope).is_err());
        let local = serde_json::to_value(
            ImageReadSnapshot::load(&images, &request, false)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(local["bytes"].is_null());
        assert_eq!(local["hasRefresh"], true);
        assert_eq!(images.inner.client.pending_read_count(), 0);
        let remote = serde_json::to_value(
            ImageReadSnapshot::load(&images, &request, true)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            remote["bytes"],
            base64::engine::general_purpose::STANDARD.encode(bitmap(7))
        );
        assert_eq!(remote["hasRefresh"], false);
        worker.join().unwrap();
        let cached = serde_json::to_value(
            ImageReadSnapshot::load(&images, &request, false)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(cached["bytes"], remote["bytes"]);
        assert_eq!(cached["hasRefresh"], false);
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn storage_failure_preserves_valid_inline_bytes_without_arbitrary_url_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let owner = Arc::new(SessionCacheOwner::new(|| {
            Err("synthetic storage failure".into())
        }));
        let _catalog = owner.session(client.clone(), None, 1);
        let images = SessionImages::new(owner.clone(), client.clone(), 1);
        let request = request(
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bitmap(3))
            ),
            None,
        );
        let load = images.read_image(&request).await.unwrap();
        assert_eq!(load.current.bytes.unwrap(), bitmap(3));
        assert!(load.current.cache_error.is_some());
        let mut external = request.clone();
        external.image.path = "https://external.invalid/cover.png".into();
        assert!(images.read_image(&external).await.is_err());
        assert_eq!(client.pending_read_count(), 0);
        owner.shutdown(1).await.unwrap();
        drop(images);
        drop(owner);
        directory.close().unwrap();
    }
}
