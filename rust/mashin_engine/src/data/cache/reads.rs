//! Shared typed routing for migrated catalog reads; media owners retain their own SQL.
//! All reads use one snapshot and all observations use the enclosing writer transaction.

use super::database::CacheError;
use super::{album_reads, artist_reads, playlist_reads, recommendation_reads, track_reads};
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::UserId;
use rusqlite::Transaction;

pub(crate) struct CatalogReadObservation {
    pub(crate) value: CatalogValue,
    pub(crate) observed_at: i64,
}

// Query routing and atomic response publication.
pub(crate) fn supports_cached_read(query: &CatalogQuery) -> bool {
    artist_reads::supports_artist_read(query)
        || album_reads::supports_album_read(query)
        || track_reads::supports_track_read(query)
        || playlist_reads::supports_playlist_read(query)
        || recommendation_reads::supports_recommendation_read(query)
}

pub(crate) fn write_cached_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    if recommendation_reads::supports_recommendation_read(query) {
        recommendation_reads::write_recommendation_read(transaction, query, user_id, value, observed_at)
    } else if playlist_reads::supports_playlist_read(query) {
        playlist_reads::write_playlist_read(transaction, query, user_id, value, observed_at)
    } else if track_reads::supports_track_read(query) {
        track_reads::write_track_read(transaction, query, user_id, value, observed_at)
    } else if album_reads::supports_album_read(query) {
        album_reads::write_album_read(transaction, query, user_id, value, observed_at)
    } else {
        artist_reads::write_artist_read(transaction, query, user_id, value, observed_at)
    }
}

// Snapshot reads preserve the media owner's exact and partial knowledge conventions.
pub(crate) fn read_cached_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    if recommendation_reads::supports_recommendation_read(query) {
        recommendation_reads::read_recommendation_read(transaction, query, user_id)
    } else if playlist_reads::supports_playlist_read(query) {
        playlist_reads::read_playlist_read(transaction, query, user_id)
    } else if track_reads::supports_track_read(query) {
        track_reads::read_track_read(transaction, query, user_id)
    } else if album_reads::supports_album_read(query) {
        album_reads::read_album_read(transaction, query, user_id)
    } else {
        artist_reads::read_artist_read(transaction, query, user_id)
    }
}

pub(crate) fn read_partial_cached(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogValue>, CacheError> {
    if recommendation_reads::supports_recommendation_read(query) {
        Ok(None)
    } else if playlist_reads::supports_playlist_read(query) {
        playlist_reads::read_partial_playlist(transaction, query, user_id)
    } else if track_reads::supports_track_read(query) {
        track_reads::read_partial_track(transaction, query, user_id)
    } else if album_reads::supports_album_read(query) {
        album_reads::read_partial_album(transaction, query, user_id)
    } else {
        artist_reads::read_partial_artist(transaction, query, user_id)
    }
}
