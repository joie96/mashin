//! Detached availability projection of visible session data.
use crate::domain::library::UserDataSnapshot;
use crate::domain::providers::{
    project_album_availability, project_fields_availability, project_item_availability,
    project_track_availability, ProviderInstance,
};
pub(crate) fn project_library_availability(
    snapshot: &mut UserDataSnapshot,
    providers: Option<&[ProviderInstance]>,
) {
    for item in &mut snapshot.favorites.tracks {
        project_track_availability(item, providers);
    }
    for item in &mut snapshot.favorites.albums {
        project_album_availability(item, providers);
    }
    for item in &mut snapshot.favorites.artists {
        project_fields_availability(&mut item.media, providers);
    }
    for item in &mut snapshot.favorites.playlists {
        project_fields_availability(&mut item.media, providers);
    }
    for playlist in &mut snapshot.playlists {
        project_fields_availability(&mut playlist.playlist.media, providers);
        for item in &mut playlist.items {
            project_track_availability(item, providers);
        }
    }
    for entry in &mut snapshot.favorite_overrides {
        project_item_availability(&mut entry.item, providers);
    }
}
