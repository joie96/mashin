//! Local images are returned before a subscriber-owned proxy refresh is polled.
//! The caller binds the client to the same observed server as the image lease.
//! Dropping a refresh or invalidating its generation cancels its network subscription;
//! accepted file writes still drain on the worker. No completed image results live here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use futures_util::future::{BoxFuture, Shared};
use futures_util::FutureExt;

use super::database::CacheError;
use super::image_files::ImagePayload;
use super::image_index::{ImageRefreshState, ImageVariantLease};
use super::image_sources::ImageVariantKey;
use super::lifecycle::ImageLease;
use crate::data::music_assistant::client::{HttpCommandError, MusicAssistantClient};
use crate::data::music_assistant::images::ImageFetch;
use crate::data::music_assistant::request_scheduler::ReadGroup;

pub(super) struct ImageRead {
    pub(super) cached: Option<ImageVariantLease>,
    pub(super) is_recently_not_found: bool,
    pub(super) cache_error: Option<CacheError>,
    pub(super) refresh: Option<BoxFuture<'static, Result<ImageRefresh, ImageRefreshError>>>,
}

#[derive(Clone)]
pub(super) enum ImageRefresh {
    Found {
        payload: ImagePayload,
        cache_error: Option<CacheError>,
    },
    NotFound {
        cache_error: Option<CacheError>,
    },
}

#[derive(Debug, Clone)]
pub(super) enum ImageRefreshError {
    Invalidated,
    Request(HttpCommandError),
    Decode(CacheError),
}

type SharedImageRefresh = Shared<BoxFuture<'static, Result<ImageRefresh, ImageRefreshError>>>;

#[derive(Default)]
pub(super) struct ImageRefreshes {
    tasks: Mutex<HashMap<(ImageVariantKey, usize), Weak<ImageRefreshTask>>>,
}

struct ImageRefreshTask {
    future: SharedImageRefresh,
    group: Arc<ReadGroup>,
    _client: Arc<MusicAssistantClient>,
}

impl ImageRefreshes {
    fn subscribe(
        &self,
        key: ImageVariantKey,
        client: Arc<MusicAssistantClient>,
        operation: BoxFuture<'static, Result<ImageRefresh, ImageRefreshError>>,
    ) -> BoxFuture<'static, Result<ImageRefresh, ImageRefreshError>> {
        let task = self
            .tasks
            .lock()
            .map_err(|_| ImageRefreshError::Decode(CacheError::WorkerUnavailable))
            .map(|mut tasks| {
                tasks.retain(|_, task| {
                    task.upgrade()
                        .is_some_and(|task| task.future.peek().is_none())
                });
                let identity = (key, Arc::as_ptr(&client) as usize);
                if let Some(task) = tasks.get(&identity).and_then(Weak::upgrade) {
                    task.group.promote_to_current_priority();
                    return task;
                }
                let group = client.create_read_group();
                let scope = group.clone();
                let task = Arc::new(ImageRefreshTask {
                    future: async move { scope.scope(operation).await }.boxed().shared(),
                    group,
                    _client: client,
                });
                tasks.insert(identity, Arc::downgrade(&task));
                task
            });
        Box::pin(async move {
            let task = task?;
            task.future.clone().await
        })
    }
}

impl ImageLease {
    // Local delivery is independent of network availability; freshness is a caller policy.
    pub(super) async fn begin_read(
        &self,
        key: ImageVariantKey,
        client: Option<Arc<MusicAssistantClient>>,
        fresh_for: Duration,
        force_refresh: bool,
    ) -> Result<ImageRead, CacheError> {
        self.require_active()?;
        let (state, cache_error) = match self.read_refresh_state(key.clone()).await {
            Ok(state) => (state, None),
            Err(CacheError::Invalidated | CacheError::ShuttingDown) => {
                return Err(CacheError::Invalidated)
            }
            Err(error) => (
                ImageRefreshState {
                    cached: None,
                    source_revision: None,
                    confirmed_at: None,
                    is_recently_not_found: false,
                },
                Some(error),
            ),
        };
        self.require_active()?;
        let ImageRefreshState {
            cached,
            source_revision,
            confirmed_at,
            is_recently_not_found,
        } = state;
        let now = chrono::Utc::now().timestamp();
        let is_fresh = cached.as_ref().is_some_and(|cached| {
            cached.read.is_current_source()
                && now
                    .checked_sub(confirmed_at.unwrap_or(cached.read.observed_at))
                    .and_then(|age| u64::try_from(age).ok())
                    .is_some_and(|age| Duration::from_secs(age) < fresh_for)
        });
        let refresh = client
            .filter(|_| force_refresh || (!is_fresh && !is_recently_not_found))
            .map(|client| {
                let images = self.clone();
                let refresh_key = key.clone();
                let refresh_client = client.clone();
                let mut invalidation = self.observe_invalidation();
                let operation = async move {
                    images
                        .require_active()
                        .map_err(|_| ImageRefreshError::Invalidated)?;
                    let response = client.fetch_proxy_image(&key, images.generation_id()).await;
                    images
                        .require_active()
                        .map_err(|_| ImageRefreshError::Invalidated)?;
                    match response.map_err(ImageRefreshError::Request)? {
                        ImageFetch::NotFound => {
                            let result = images.record_not_found(key, source_revision).await;
                            images
                                .require_active()
                                .map_err(|_| ImageRefreshError::Invalidated)?;
                            Ok(ImageRefresh::NotFound {
                                cache_error: result.err(),
                            })
                        }
                        ImageFetch::Found(bytes) => {
                            let payload = images
                                .validate_bytes(bytes.to_vec())
                                .await
                                .map_err(decode_error)?;
                            let result = images
                                .publish_variant(
                                    key,
                                    payload.bytes.clone(),
                                    chrono::Utc::now().timestamp(),
                                )
                                .await;
                            images
                                .require_active()
                                .map_err(|_| ImageRefreshError::Invalidated)?;
                            Ok(ImageRefresh::Found {
                                payload,
                                cache_error: result.err(),
                            })
                        }
                    }
                };
                let operation = Box::pin(async move {
                    tokio::select! {
                        biased;
                        _ = async {
                            while !*invalidation.borrow() {
                                if invalidation.changed().await.is_err() { break }
                            }
                        } => Err(ImageRefreshError::Invalidated),
                        result = operation => result,
                    }
                })
                    as BoxFuture<'static, Result<ImageRefresh, ImageRefreshError>>;
                self.refreshes
                    .subscribe(refresh_key, refresh_client, operation)
            });
        Ok(ImageRead {
            cached,
            is_recently_not_found,
            cache_error,
            refresh,
        })
    }
}

fn decode_error(error: CacheError) -> ImageRefreshError {
    match error {
        CacheError::Invalidated | CacheError::ShuttingDown => ImageRefreshError::Invalidated,
        error => ImageRefreshError::Decode(error),
    }
}

#[cfg(test)]
mod tests {
    use super::super::image_files::tests::bitmap;
    use super::super::image_sources::{
        ImageOutputFormat, ImageSize, ImageSourceKey, ImageVariantParameters,
    };
    use super::super::lifecycle::ServerCache;
    use super::super::storage::{CacheStorage, IMAGE_DATABASE_FILE_NAME};
    use super::*;
    use crate::data::music_assistant::images::tests::server_with_request_handler;

    fn key() -> ImageVariantKey {
        ImageVariantKey {
            source: ImageSourceKey::ProxyId("a".repeat(64)),
            parameters: ImageVariantParameters {
                size: ImageSize::Pixels80,
                output_format: ImageOutputFormat::Png,
            },
        }
    }

    async fn cache(path: &std::path::Path) -> ServerCache {
        let cache = ServerCache::initialize(path.to_owned()).await.unwrap();
        cache.activate("server".into()).await.unwrap();
        cache
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_coordination_separates_clients_variants_and_releases_last_consumer() {
        use crate::application::repositories::{with_request_priority, RequestPriority};
        let refreshes = ImageRefreshes::default();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let operation = || {
            Box::pin(std::future::pending())
                as BoxFuture<'static, Result<ImageRefresh, ImageRefreshError>>
        };
        let first = with_request_priority(RequestPriority::Background, async {
            refreshes.subscribe(key(), client.clone(), operation())
        })
        .await;
        let task = refreshes
            .tasks
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .upgrade()
            .unwrap();
        assert_eq!(task.group.effective_priority(), RequestPriority::Background);
        let second = refreshes.subscribe(key(), client.clone(), operation());
        assert_eq!(task.group.effective_priority(), RequestPriority::Foreground);
        let mut sibling = key();
        sibling.parameters.size = ImageSize::Pixels512;
        let third = refreshes.subscribe(sibling, client, operation());
        let other_client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let fourth = refreshes.subscribe(key(), other_client, operation());
        assert_eq!(refreshes.tasks.lock().unwrap().len(), 3);
        drop(task);
        drop((first, second, third, fourth));
        assert!(refreshes
            .tasks
            .lock()
            .unwrap()
            .values()
            .all(|task| task.strong_count() == 0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_refreshes_share_processing_and_survive_one_subscriber_drop() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let (client, worker) = server_with_request_handler(200, bitmap(2), move || {
            started.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let client = Arc::new(client);
        let mut first = images
            .begin_read(key(), Some(client.clone()), Duration::ZERO, false)
            .await
            .unwrap();
        let mut second = images
            .begin_read(key(), Some(client), Duration::ZERO, false)
            .await
            .unwrap();
        let shared = {
            let tasks = images.refreshes.tasks.lock().unwrap();
            assert_eq!(tasks.len(), 1);
            let task = tasks.values().next().unwrap();
            assert_eq!(task.strong_count(), 2);
            task.clone()
        };
        let canceled = tokio::spawn(first.refresh.take().unwrap());
        start.await.unwrap();
        canceled.abort();
        assert!(matches!(canceled.await, Err(error) if error.is_cancelled()));
        assert_eq!(shared.strong_count(), 1);
        let remaining = tokio::spawn(second.refresh.take().unwrap());
        release.send(()).unwrap();
        let result = remaining.await.unwrap().unwrap();
        assert!(matches!(
            result,
            ImageRefresh::Found {
                cache_error: None,
                ..
            }
        ));
        assert_eq!(shared.strong_count(), 0);
        assert_eq!(
            images
                .read_variant(key())
                .await
                .unwrap()
                .unwrap()
                .read
                .payload
                .bytes,
            bitmap(2)
        );
        worker.join().unwrap();
        drop(first);
        drop(second);
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_delivery_precedes_delayed_proxy_and_new_bytes_survive_offline_restart() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        images.publish_variant(key(), bitmap(1), 1).await.unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let (client, worker) = server_with_request_handler(200, bitmap(2), move || {
            started.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let mut read = images
            .begin_read(key(), Some(Arc::new(client)), Duration::ZERO, false)
            .await
            .unwrap();
        assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        assert!(read.cache_error.is_none());
        let refresh = tokio::spawn(read.refresh.take().unwrap());
        start.await.unwrap();
        assert!(!refresh.is_finished());
        assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        release.send(()).unwrap();
        let ImageRefresh::Found {
            payload,
            cache_error,
        } = refresh.await.unwrap().unwrap()
        else {
            panic!("expected updated image")
        };
        assert_eq!(payload.bytes, bitmap(2));
        assert_eq!(payload.mime_type, "image/png");
        assert!(cache_error.is_none());
        worker.join().unwrap();
        drop(read);
        cache.shutdown().await.unwrap();
        drop(cache);
        let reopened = self::cache(directory.path()).await;
        let read = reopened
            .images()
            .await
            .unwrap()
            .begin_read(key(), None, Duration::ZERO, false)
            .await
            .unwrap();
        assert!(read.refresh.is_none());
        assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(2));
        drop(read);
        reopened.shutdown().await.unwrap();
        drop(reopened);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_failures_preserve_cached_bytes_and_remain_distinguishable() {
        for (status, bytes) in [
            (404, Vec::new()),
            (503, Vec::new()),
            (200, b"invalid image".to_vec()),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let images = cache.images().await.unwrap();
            images.publish_variant(key(), bitmap(3), 1).await.unwrap();
            let (client, worker) = server_with_request_handler(status, bytes, || {});
            let mut read = images
                .begin_read(key(), Some(Arc::new(client)), Duration::ZERO, true)
                .await
                .unwrap();
            let result = read.refresh.take().unwrap().await;
            match status {
                404 => assert!(matches!(
                    result,
                    Ok(ImageRefresh::NotFound { cache_error: None })
                )),
                503 => assert!(matches!(
                    result,
                    Err(ImageRefreshError::Request(HttpCommandError::Http {
                        status: 503,
                        ..
                    }))
                )),
                _ => assert!(matches!(result, Err(ImageRefreshError::Decode(_)))),
            }
            let retry = images
                .begin_read(
                    key(),
                    Some(Arc::new(MusicAssistantClient::new(
                        "http://127.0.0.1:1",
                        None,
                    ))),
                    Duration::ZERO,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(retry.is_recently_not_found, status == 404);
            assert_eq!(retry.refresh.is_none(), status == 404);
            drop(retry);
            assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(3));
            assert_eq!(
                images
                    .read_variant(key())
                    .await
                    .unwrap()
                    .unwrap()
                    .read
                    .payload
                    .bytes,
                bitmap(3)
            );
            worker.join().unwrap();
            drop(read);
            cache.shutdown().await.unwrap();
            drop(cache);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn not_found_suppresses_retries_preserves_local_bytes_and_force_can_replace_it() {
        for has_local_image in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let images = cache.images().await.unwrap();
            if has_local_image {
                images.publish_variant(key(), bitmap(1), 1).await.unwrap();
            }
            let (client, worker) = server_with_request_handler(404, Vec::new(), || {});
            let mut read = images
                .begin_read(key(), Some(Arc::new(client)), Duration::ZERO, false)
                .await
                .unwrap();
            assert!(matches!(
                read.refresh.take().unwrap().await,
                Ok(ImageRefresh::NotFound { cache_error: None })
            ));
            worker.join().unwrap();
            drop(read);
            cache.clear_database_cache().await.unwrap();
            let unavailable = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
            for client in [None, Some(unavailable.clone())] {
                let read = images
                    .begin_read(key(), client, Duration::ZERO, false)
                    .await
                    .unwrap();
                assert!(read.is_recently_not_found);
                assert!(read.refresh.is_none());
                assert_eq!(read.cached.is_some(), has_local_image);
                if has_local_image {
                    assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
                }
            }
            let (client, worker) = server_with_request_handler(200, bitmap(2), || {});
            let mut read = images
                .begin_read(key(), Some(Arc::new(client)), Duration::ZERO, true)
                .await
                .unwrap();
            assert!(matches!(
                read.refresh.take().unwrap().await,
                Ok(ImageRefresh::Found {
                    cache_error: None,
                    ..
                })
            ));
            worker.join().unwrap();
            drop(read);
            let read = images
                .begin_read(key(), Some(unavailable), Duration::ZERO, false)
                .await
                .unwrap();
            assert!(!read.is_recently_not_found);
            assert!(read.refresh.is_some());
            assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(2));
            drop(read);
            cache.shutdown().await.unwrap();
            drop(cache);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn late_not_found_cannot_suppress_refresh_of_a_new_source_revision() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        let mut sibling = key();
        sibling.parameters.size = ImageSize::Pixels512;
        images.publish_variant(key(), bitmap(1), 1).await.unwrap();
        images
            .publish_variant(sibling.clone(), bitmap(1), 1)
            .await
            .unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let (client, worker) = server_with_request_handler(404, Vec::new(), move || {
            started.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let mut read = images
            .begin_read(key(), Some(Arc::new(client)), Duration::ZERO, false)
            .await
            .unwrap();
        let refresh = tokio::spawn(read.refresh.take().unwrap());
        start.await.unwrap();
        images.publish_variant(sibling, bitmap(2), 2).await.unwrap();
        release.send(()).unwrap();
        assert!(matches!(
            refresh.await.unwrap(),
            Ok(ImageRefresh::NotFound { cache_error: None })
        ));
        worker.join().unwrap();
        drop(read);
        let read = images
            .begin_read(
                key(),
                Some(Arc::new(MusicAssistantClient::new(
                    "http://127.0.0.1:1",
                    None,
                ))),
                Duration::from_secs(300),
                false,
            )
            .await
            .unwrap();
        assert!(!read.is_recently_not_found);
        assert!(read.refresh.is_some());
        assert!(!read.cached.as_ref().unwrap().read.is_current_source());
        assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        drop(read);
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn svg_proxy_variant_persists_png_and_rejects_active_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        let mut variant = key();
        variant.parameters.output_format = ImageOutputFormat::Svg;
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><path fill="red" d="M0 0H2V2H0Z"/></svg>"#.to_vec();
        let (client, worker) = server_with_request_handler(200, svg, || {});
        let mut read = images
            .begin_read(
                variant.clone(),
                Some(Arc::new(client)),
                Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        let ImageRefresh::Found {
            payload,
            cache_error,
        } = read.refresh.take().unwrap().await.unwrap()
        else {
            panic!("expected normalized proxy image")
        };
        assert_eq!(payload.mime_type, "image/png");
        assert_eq!(
            image::load_from_memory(&payload.bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255]
        );
        assert!(cache_error.is_none());
        assert!(worker
            .join()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .contains("fmt=svg"));
        drop(read);
        let (client, worker) = server_with_request_handler(
            200,
            br#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#.to_vec(),
            || {},
        );
        let mut read = images
            .begin_read(
                variant.clone(),
                Some(Arc::new(client)),
                Duration::ZERO,
                true,
            )
            .await
            .unwrap();
        assert!(matches!(
            read.refresh.take().unwrap().await,
            Err(ImageRefreshError::Decode(_))
        ));
        assert_eq!(
            read.cached.as_ref().unwrap().read.payload.bytes,
            payload.bytes
        );
        worker.join().unwrap();
        drop(read);
        cache.shutdown().await.unwrap();
        drop(cache);
        let reopened = self::cache(directory.path()).await;
        let read = reopened
            .images()
            .await
            .unwrap()
            .begin_read(variant, None, Duration::ZERO, false)
            .await
            .unwrap();
        assert_eq!(
            read.cached.as_ref().unwrap().read.payload.bytes,
            payload.bytes
        );
        assert_eq!(
            read.cached.as_ref().unwrap().read.payload.mime_type,
            "image/png"
        );
        assert!(read.refresh.is_none());
        drop(read);
        reopened.shutdown().await.unwrap();
        drop(reopened);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn index_failure_does_not_discard_valid_proxy_bytes() {
        for status in [200, 404] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let storage = CacheStorage::open(directory.path()).unwrap();
            let index = storage.image_directory().join(IMAGE_DATABASE_FILE_NAME);
            std::fs::write(&index, b"foreign index").unwrap();
            drop(storage);
            let (client, worker) = server_with_request_handler(status, bitmap(4), || {});
            let mut read = cache
                .images()
                .await
                .unwrap()
                .begin_read(key(), Some(Arc::new(client)), Duration::ZERO, false)
                .await
                .unwrap();
            assert!(read.cached.is_none());
            assert!(read.cache_error.is_some());
            match read.refresh.take().unwrap().await.unwrap() {
                ImageRefresh::Found {
                    payload,
                    cache_error,
                } => {
                    assert_eq!(status, 200);
                    assert_eq!(payload.bytes, bitmap(4));
                    assert!(cache_error.is_some());
                }
                ImageRefresh::NotFound { cache_error } => {
                    assert_eq!(status, 404);
                    assert!(cache_error.is_some());
                }
            }
            assert_eq!(std::fs::read(index).unwrap(), b"foreign index");
            worker.join().unwrap();
            drop(read);
            cache.shutdown().await.unwrap();
            drop(cache);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_clear_cancels_live_refresh_and_never_repopulates_new_generation() {
        for status in [200, 404] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let images = cache.images().await.unwrap();
            let generation = images.generation_id();
            let (started, start) = tokio::sync::oneshot::channel();
            let (release, hold) = std::sync::mpsc::channel();
            let (client, worker) = server_with_request_handler(status, bitmap(5), move || {
                started.send(()).unwrap();
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            let client = Arc::new(client);
            let mut read = images
                .begin_read(key(), Some(client.clone()), Duration::ZERO, false)
                .await
                .unwrap();
            let refresh = tokio::spawn(read.refresh.take().unwrap());
            start.await.unwrap();
            cache.clear_image_cache().await.unwrap();
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), refresh)
                    .await
                    .unwrap()
                    .unwrap(),
                Err(ImageRefreshError::Invalidated)
            ));
            assert_eq!(client.pending_read_count(), 0);
            let current = cache.images().await.unwrap();
            assert_ne!(generation, current.generation_id());
            assert!(current.read_variant(key()).await.unwrap().is_none());
            release.send(()).unwrap();
            worker.join().unwrap();
            assert!(current.read_variant(key()).await.unwrap().is_none());
            assert!(
                !current
                    .begin_read(key(), None, Duration::ZERO, false)
                    .await
                    .unwrap()
                    .is_recently_not_found
            );
            drop(read);
            cache.shutdown().await.unwrap();
            drop(cache);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn identical_proxy_refresh_renews_freshness_without_changing_persisted_observation() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        let mut sibling = key();
        sibling.parameters.size = ImageSize::Pixels512;
        images.publish_variant(key(), bitmap(1), 1).await.unwrap();
        images
            .publish_variant(sibling.clone(), bitmap(2), 1)
            .await
            .unwrap();
        let (client, worker) = server_with_request_handler(200, bitmap(1), || {});
        let mut read = images
            .begin_read(
                key(),
                Some(Arc::new(client)),
                Duration::from_secs(300),
                false,
            )
            .await
            .unwrap();
        assert_eq!(read.cached.as_ref().unwrap().read.observed_at, 1);
        let ImageRefresh::Found {
            payload,
            cache_error,
        } = read.refresh.take().unwrap().await.unwrap()
        else {
            panic!("expected identical proxy confirmation")
        };
        assert_eq!(payload.bytes, bitmap(1));
        assert!(cache_error.is_none());
        worker.join().unwrap();
        drop(read);
        cache.clear_database_cache().await.unwrap();
        let unavailable = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        for (variant, fresh_for, force_refresh, should_refresh) in [
            (key(), Duration::from_secs(300), false, false),
            (key(), Duration::from_secs(300), true, true),
            (sibling, Duration::from_secs(300), false, true),
            (key(), Duration::ZERO, false, true),
        ] {
            let read = images
                .begin_read(variant, Some(unavailable.clone()), fresh_for, force_refresh)
                .await
                .unwrap();
            assert_eq!(read.refresh.is_some(), should_refresh);
            assert!(!read.is_recently_not_found);
            assert_eq!(read.cached.as_ref().unwrap().read.observed_at, 1);
        }
        let (client, worker) = server_with_request_handler(503, Vec::new(), || {});
        let mut read = images
            .begin_read(
                key(),
                Some(Arc::new(client)),
                Duration::from_secs(300),
                true,
            )
            .await
            .unwrap();
        assert!(matches!(
            read.refresh.take().unwrap().await,
            Err(ImageRefreshError::Request(_))
        ));
        worker.join().unwrap();
        drop(read);
        let read = images
            .begin_read(
                key(),
                Some(unavailable.clone()),
                Duration::from_secs(300),
                false,
            )
            .await
            .unwrap();
        assert!(read.refresh.is_none());
        assert_eq!(read.cached.as_ref().unwrap().read.observed_at, 1);
        drop(read);
        cache.shutdown().await.unwrap();
        drop(cache);
        let reopened = self::cache(directory.path()).await;
        let read = reopened
            .images()
            .await
            .unwrap()
            .begin_read(key(), Some(unavailable), Duration::from_secs(300), false)
            .await
            .unwrap();
        assert_eq!(read.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        assert_eq!(read.cached.as_ref().unwrap().read.observed_at, 1);
        assert!(read.refresh.is_none());
        drop(read);
        reopened.shutdown().await.unwrap();
        drop(reopened);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fresh_images_skip_network_unless_forced_and_stale_variants_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        let now = chrono::Utc::now().timestamp();
        images.publish_variant(key(), bitmap(6), now).await.unwrap();
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let read = images
            .begin_read(key(), Some(client.clone()), Duration::from_secs(300), false)
            .await
            .unwrap();
        assert!(read.refresh.is_none());
        assert!(read.cached.is_some());
        drop(read);
        let read = images
            .begin_read(key(), Some(client.clone()), Duration::from_secs(300), true)
            .await
            .unwrap();
        assert!(read.refresh.is_some());
        drop(read);
        let mut neighbor = key();
        neighbor.parameters.size = ImageSize::Pixels160;
        images
            .publish_variant(neighbor.clone(), bitmap(6), now)
            .await
            .unwrap();
        images.publish_variant(key(), bitmap(7), now).await.unwrap();
        let read = images
            .begin_read(
                neighbor,
                Some(client.clone()),
                Duration::from_secs(300),
                false,
            )
            .await
            .unwrap();
        assert!(!read.cached.as_ref().unwrap().read.is_current_source());
        assert!(read.refresh.is_some());
        drop(read);
        assert_eq!(client.pending_read_count(), 0);
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn last_refresh_consumer_drop_cancels_proxy_without_persisting_late_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let images = cache.images().await.unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let (client, worker) = server_with_request_handler(200, bitmap(8), move || {
            started.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let client = Arc::new(client);
        let mut read = images
            .begin_read(key(), Some(client.clone()), Duration::ZERO, false)
            .await
            .unwrap();
        let refresh = tokio::spawn(read.refresh.take().unwrap());
        start.await.unwrap();
        refresh.abort();
        assert!(matches!(refresh.await, Err(error) if error.is_cancelled()));
        assert_eq!(client.pending_read_count(), 0);
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(images.read_variant(key()).await.unwrap().is_none());
        drop(read);
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }
}
