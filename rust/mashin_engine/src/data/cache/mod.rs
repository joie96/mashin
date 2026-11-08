//! Server-bound cache persistence, separate from credentials and configuration.
//! Database connections belong on blocking workers, never on the async executor or UI thread.
//! Session catalogs expose migrated media observations before their shared MA refresh completes.

pub(super) mod album_artist_relations;
pub(super) mod album_candidates;
pub(crate) mod album_reads;
pub(super) mod albums;
pub(super) mod artist_candidates;
pub(crate) mod artist_reads;
pub(super) mod artists;
pub(crate) mod catalog;
mod catalog_sections;
pub(super) mod database;
pub(super) mod image_files;
pub(crate) mod lifecycle;
pub(super) mod playlist_reads;
pub(super) mod playlists;
pub(crate) mod reads;
pub(super) mod recommendation_reads;
pub(super) mod storage;
pub(crate) mod track_reads;
pub(super) mod tracks;
pub(super) mod workers;
