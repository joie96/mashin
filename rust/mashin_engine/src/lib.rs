#![allow(linker_messages)]

//! C FFI and Android JNI facade for the playback core.

mod adapters;
mod application;
mod audio;
mod bootstrap;
mod configuration;
mod credentials;
mod data;
mod domain;
mod logging;
mod playback;

#[cfg(test)]
mod architecture_tests {
    fn assert_boundary(name: &str, source: &str, forbidden: &[&str]) {
        let normalized = source.replace("\r\n", "\n");
        let production = normalized.split("\n#[cfg(test)]\nmod ").next().unwrap();
        let production = production
            .split("\n#[cfg(test)]\npub(crate) mod ")
            .next()
            .unwrap();
        let production = production
            .split("\n#[cfg(test)]\npub(super) mod ")
            .next()
            .unwrap();
        for dependency in forbidden {
            assert!(
                !production.contains(dependency),
                "{name} depends on {dependency}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "probe depends on forbidden")]
    fn test_import_attributes_do_not_hide_production_dependencies() {
        assert_boundary(
            "probe",
            "#[cfg(test)]\nuse test_helper;\nfn production() { forbidden(); }",
            &["forbidden"],
        );
    }

    #[test]
    fn source_tree_has_no_obsolete_module_roots() {
        let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for name in ["core", "ffi", "jni", "music_assistant"] {
            for path in [
                source_root.join(name),
                source_root.join(format!("{name}.rs")),
            ] {
                assert!(
                    !path.exists(),
                    "obsolete source root remains: {}",
                    path.display()
                );
            }
        }
        for name in ["data/cache", "data/repositories", "data/image_loader.rs"] {
            assert!(
                !source_root.join(name).exists(),
                "removed cache path remains: {name}"
            );
        }
    }

    #[test]
    fn media_use_cases_use_repository_ports_without_root_compatibility_paths() {
        assert_boundary(
            "registered roots",
            include_str!("lib.rs"),
            &["mod core;", "mod music_assistant;"],
        );
        for (name, source) in [
            (
                "recommendations",
                include_str!("application/recommendations.rs"),
            ),
            ("artwork", include_str!("application/playlist_artwork.rs")),
            ("radio", include_str!("application/radio.rs")),
            ("playback", include_str!("application/playback.rs")),
            ("startup", include_str!("application/startup.rs")),
            ("queries", include_str!("application/queries.rs")),
        ] {
            assert_boundary(
                name,
                source,
                &[
                    "crate::data::",
                    "crate::core::",
                    "crate::music_assistant::",
                    "MusicAssistantClient",
                    ".execute_command(",
                ],
            );
        }
        assert_boundary(
            "transport",
            include_str!("data/music_assistant/client.rs"),
            &[
                "catalog_repository:",
                "initialize_catalog_repository",
                "artwork_cache",
                "RecommendationData",
                "fn load_initial_",
                "fn get_playlist_tracks(",
            ],
        );
        assert_boundary(
            "library transport",
            include_str!("data/music_assistant/library.rs"),
            &["fn load_initial_", "fn get_playlist_tracks("],
        );
        assert_boundary(
            "domain media mapping",
            include_str!("domain/media.rs"),
            &[
                "deserialize_album_year",
                "include_mapping_image",
                "PodcastInput",
            ],
        );
        assert_boundary(
            "domain recommendation mapping",
            include_str!("domain/recommendations.rs"),
            &["alias =", "deserialize_nullable_vec"],
        );
        assert_boundary(
            "generic transport",
            include_str!("data/music_assistant/client.rs"),
            &[
                "fn get_library_page",
                "fn set_favorite",
                "fn create_private_playlist",
                "fn execute_queue_command",
                "fn execute_player_command",
                "fn login(",
            ],
        );
    }

    #[test]
    fn playback_uses_domain_values_and_injected_server_ports() {
        let modules = [
            ("backend", include_str!("playback/backend.rs")),
            ("engine", include_str!("playback/engine.rs")),
            ("models", include_str!("playback/models.rs")),
            ("mode", include_str!("playback/mode.rs")),
            ("runtime", include_str!("playback/runtime.rs")),
            ("start", include_str!("playback/start.rs")),
            ("server", include_str!("playback/server.rs")),
            ("target", include_str!("playback/server_target.rs")),
            ("queue", include_str!("playback/queue_controller.rs")),
            ("projection", include_str!("playback/queue_projection.rs")),
            ("local", include_str!("playback/local/playback.rs")),
            ("session", include_str!("playback/local/session.rs")),
            (
                "receiver",
                include_str!("playback/local/mashin/receiver.rs"),
            ),
            (
                "protocol",
                include_str!("playback/local/mashin/protocol.rs"),
            ),
            (
                "stream models",
                include_str!("playback/local/mashin/models.rs"),
            ),
            (
                "audio adapter",
                include_str!("playback/local/mashin/audio_adapter.rs"),
            ),
            ("streams", include_str!("playback/local/mashin/streams.rs")),
            (
                "recovery",
                include_str!("playback/local/mashin/recovery.rs"),
            ),
            ("remote", include_str!("playback/remote/playback.rs")),
        ];
        for (name, source) in modules {
            assert_boundary(
                name,
                source,
                &[
                    "crate::core::",
                    "crate::music_assistant::",
                    "crate::data::",
                    "crate::adapters::",
                    "crate::bootstrap::",
                ],
            );
        }
    }

    #[test]
    fn library_use_cases_do_not_import_concrete_data_adapters() {
        for (name, source) in [
            ("library", include_str!("application/library.rs")),
            ("favorites", include_str!("application/favorites.rs")),
            ("snapshots", include_str!("application/snapshots.rs")),
            (
                "repository contracts",
                include_str!("application/repositories.rs"),
            ),
        ] {
            assert_boundary(
                name,
                source,
                &[
                    "crate::data::",
                    "crate::music_assistant::",
                    "crate::adapters::",
                    "crate::bootstrap::",
                ],
            );
        }
    }

    #[test]
    fn providers_have_no_global_token_cache_or_hidden_transport_enrichment() {
        assert_boundary(
            "MA client",
            include_str!("data/music_assistant/client.rs"),
            &[
                "PROVIDER_AVAILABILITY",
                "ProviderAvailabilitySnapshot",
                "provider_availability(",
                "annotate_media_availability",
                "get_provider_icon_data",
                "get_provider_manifests",
            ],
        );
        assert_boundary(
            "provider transport",
            include_str!("data/music_assistant/session.rs"),
            &["static ", "HashMap", "Mutex"],
        );
    }

    #[test]
    fn catalog_reads_have_one_transport_owner_and_no_repository_result_cache() {
        assert_boundary(
            "MA client",
            include_str!("data/music_assistant/client.rs"),
            &[
                "fn get_artist(",
                "fn get_album(",
                "fn get_track(",
                "fn search_media(",
                "fn get_recently_played_tracks(",
                "fn get_artist_albums(",
                "fn get_track_versions(",
                "fn get_similar_artists(",
                "fn get_similar_tracks(",
                "fn get_album_tracks(",
                "fn get_artist_top_tracks(",
                "fn get_artist_tracks(",
            ],
        );
        assert_boundary(
            "MA client playlist metadata",
            include_str!("data/music_assistant/client.rs"),
            &[
                "fn get_playlist(",
                "item_reference_arguments",
                "\"music/playlists/get\"",
                "\"music/playlists/playlist_tracks\"",
            ],
        );
        assert_boundary(
            "catalog transport",
            include_str!("data/music_assistant/catalog.rs"),
            &["static ", "HashMap", "Mutex"],
        );
        assert_boundary(
            "dispatcher catalog playlist reads",
            include_str!("application/dispatcher.rs"),
            &[".get_playlist_tracks(", "apply_playlist_tracks"],
        );
        assert_boundary(
            "recommendation track ownership",
            include_str!("application/recommendations.rs"),
            &[
                "CachedPlaylistTracks",
                "HashMap",
                "Mutex<Option<Vec<Track>>>",
                "folders: Vec<RecommendationFolder>",
            ],
        );
        assert_boundary(
            "recommendation application transport",
            include_str!("application/recommendations.rs")
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap(),
            &["crate::data::", "execute_command", "MusicAssistantClient"],
        );
        assert_boundary(
            "client recommendation state",
            include_str!("data/music_assistant/client.rs"),
            &["RecommendationData", "recommendation_data:"],
        );
        assert_boundary(
            "radio catalog reads",
            include_str!("application/radio.rs"),
            &[
                "MusicAssistantClient",
                "execute_command",
                ".get_playlist_tracks(",
            ],
        );
    }

    #[test]
    fn domain_values_do_not_depend_on_application_or_platform_owners() {
        for (name, source) in [
            ("settings", include_str!("domain/settings.rs")),
            ("session", include_str!("domain/session.rs")),
            ("identifiers", include_str!("domain/identifiers.rs")),
            ("media", include_str!("domain/media.rs")),
            ("favorites", include_str!("domain/favorites.rs")),
            ("library", include_str!("domain/library.rs")),
            ("providers", include_str!("domain/providers.rs")),
            ("catalog", include_str!("domain/catalog.rs")),
            ("recommendations", include_str!("domain/recommendations.rs")),
            ("artwork", include_str!("domain/artwork.rs")),
            ("radio", include_str!("domain/radio.rs")),
        ] {
            assert_boundary(
                name,
                source,
                &[
                    "crate::core::",
                    "crate::application::",
                    "crate::music_assistant::",
                    "crate::data::",
                    "crate::playback::",
                    "crate::audio::",
                    "crate::adapters::",
                    "crate::bootstrap::",
                ],
            );
        }
    }

    #[test]
    fn persistence_and_core_receive_platform_capabilities() {
        for (name, source) in [
            ("configuration", include_str!("configuration.rs")),
            ("logging", include_str!("logging.rs")),
            ("credentials", include_str!("credentials/session_store.rs")),
            ("application state", include_str!("application/state.rs")),
        ] {
            assert_boundary(
                name,
                source,
                &[
                    "crate::adapters::",
                    "crate::bootstrap::",
                    "std::env::",
                    "static RUNTIME",
                    "static COORDINATOR",
                    "MusicAssistantConnection::new",
                ],
            );
        }
    }
}
