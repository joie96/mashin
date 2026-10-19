//! Application-core ownership, models, commands, and worker lifecycle.

pub(crate) mod config;
pub(crate) mod core;
pub(crate) mod event_channel;
pub(crate) mod models;

pub(crate) use core::{
    add_playlist_tracks, add_private_playlist_tracks, create_private_playlist,
    execute_command_and_collect_events, execute_playback_command_and_collect_events,
    read_media_session_state, read_music_assistant_album, read_music_assistant_album_tracks,
    read_music_assistant_artist, read_music_assistant_artist_albums,
    read_music_assistant_artist_top_tracks, read_music_assistant_playlist,
    read_music_assistant_playlist_tracks, read_music_assistant_provider_icon_data,
    read_music_assistant_provider_manifests, read_music_assistant_recently_played_tracks,
    read_music_assistant_recommendations, read_music_assistant_similar_artists,
    read_music_assistant_similar_tracks, read_music_assistant_track_versions,
    read_or_load_user_data, read_player_state, read_queue, read_session_settings, read_user_data,
    refresh_private_playlist_cover, remove_playlist_tracks, remove_private_playlist,
    remove_private_playlist_tracks, rename_private_playlist, resolve_media_and_execute_command,
    restore_persisted_session_and_collect_events, search_music_assistant_media,
    set_user_data_favorites, shutdown, spawn_blocking_operation, start_music_assistant_query,
    start_radio_and_execute_command, CoreCoordinator,
};
