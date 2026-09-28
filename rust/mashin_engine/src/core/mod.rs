//! Application-core ownership, models, commands, and worker lifecycle.

pub(crate) mod config;
pub(crate) mod core;
pub(crate) mod models;

pub(crate) use core::{
    add_playlist_tracks, execute_command_and_collect_events, mutate_local_playlist_tracks,
    mutate_user_data, read_media_session_state, read_music_assistant_album,
    read_music_assistant_album_tracks, read_music_assistant_artist,
    read_music_assistant_artist_albums, read_music_assistant_artist_top_tracks,
    read_music_assistant_playlist, read_music_assistant_playlist_tracks,
    read_music_assistant_provider_icon_data, read_music_assistant_provider_manifests,
    read_music_assistant_recently_played_tracks, read_music_assistant_recommendations,
    read_music_assistant_similar_artists, read_music_assistant_similar_tracks,
    read_music_assistant_track_versions, read_or_load_user_data, read_player_state, read_queue,
    read_session_settings, read_user_data, remove_playlist_tracks,
    resolve_media_and_execute_command, restore_persisted_session_and_collect_events,
    search_music_assistant, shutdown, spawn_blocking_operation, start_music_assistant_query,
    start_sendspin_radio_and_execute_command, CoreCoordinator,
};
