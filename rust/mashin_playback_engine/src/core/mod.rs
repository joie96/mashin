//! Application-core ownership, models, commands, and worker lifecycle.

pub(crate) mod config;
pub(crate) mod core;
pub(crate) mod models;

pub(crate) use core::{
    add_playlist_tracks, execute_command_and_collect_events, load_user_data, mutate_user_data,
    read_player_state, read_queue, read_session_settings, read_user_data, remove_playlist_tracks,
    resolve_media_and_execute_command, resolve_sendspin_radio_and_execute_command,
    restore_persisted_session_and_collect_events, shutdown, start_music_assistant_query,
    CoreCoordinator,
};
