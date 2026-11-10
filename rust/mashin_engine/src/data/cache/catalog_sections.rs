//! Catalog sections own ordered references and occurrence context, never media metadata.
//! The enclosing media read owns user/context/time and the writer transaction.

use rusqlite::{params, Transaction};

use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{
    FieldKnowledge, MediaItem, MediaItemImage, MediaType, MusicAssistantSearchResults,
};

use super::albums::write_album_observation;
use super::database::CacheError;
use super::tracks::{read_track, write_track_observation};

// Schema and transactional observations.
pub(super) fn initialize_artist_section_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE artist_album_read_items (
            user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),
            provider TEXT NOT NULL,item_id TEXT NOT NULL,position INTEGER CHECK(position>=0),
            position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),PRIMARY KEY(user_id,query,occurrence),
            FOREIGN KEY(user_id,query) REFERENCES artist_read_states(user_id,query) ON DELETE CASCADE,
            FOREIGN KEY(provider,item_id) REFERENCES albums(provider,item_id)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE artist_track_read_items (
            user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),
            provider TEXT NOT NULL,item_id TEXT NOT NULL,album_provider TEXT,album_item_id TEXT,
            album_known INTEGER NOT NULL CHECK(album_known IN (0,1)),
            disc_number INTEGER CHECK(disc_number>=0),track_number INTEGER CHECK(track_number>=0),
            position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
            PRIMARY KEY(user_id,query,occurrence),CHECK((album_provider IS NULL)=(album_item_id IS NULL)),
            CHECK(album_known=1 OR album_provider IS NULL),
            FOREIGN KEY(user_id,query) REFERENCES artist_read_states(user_id,query) ON DELETE CASCADE,
            FOREIGN KEY(provider,item_id) REFERENCES tracks(provider,item_id),
            FOREIGN KEY(album_provider,album_item_id,provider,item_id)
                REFERENCES album_tracks(album_provider,album_item_id,track_provider,track_item_id)
         ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

#[derive(Clone, Default)]
struct TrackOccurrenceContext {
    album: FieldKnowledge<Option<(ProviderInstanceId, MediaId)>>,
    disc_number: Option<u16>,
    track_number: Option<u16>,
    image: FieldKnowledge<Option<MediaItemImage>>,
}

struct SectionOccurrence {
    provider: ProviderInstanceId,
    item_id: MediaId,
    position: FieldKnowledge<Option<u32>>,
    context: TrackOccurrenceContext,
}

pub(super) fn write_catalog_section(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
) -> Result<(), CacheError> {
    let items = match (query, value) {
        (
            CatalogQuery::Search {
                media_type: MediaType::Track,
                ..
            },
            CatalogValue::Search(results),
        ) => results
            .tracks
            .iter()
            .cloned()
            .map(MediaItem::Track)
            .collect(),
        (CatalogQuery::ArtistAlbums { .. }, CatalogValue::Albums(albums)) => albums
            .iter()
            .cloned()
            .map(MediaItem::Album)
            .collect::<Vec<_>>(),
        (
            CatalogQuery::ArtistTracks { .. }
            | CatalogQuery::ArtistTopTracks { .. }
            | CatalogQuery::AlbumTracks { .. }
            | CatalogQuery::TrackVersions { .. }
            | CatalogQuery::SimilarTracks { .. }
            | CatalogQuery::RecentlyPlayed { .. }
            | CatalogQuery::LibraryTracks { .. }
            | CatalogQuery::PlaylistTracks { .. },
            CatalogValue::Tracks(tracks),
        ) => tracks
            .iter()
            .cloned()
            .map(MediaItem::Track)
            .collect::<Vec<_>>(),
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "catalog section query and value do not match",
            ))
        }
    };
    let table = section_table(query)?;
    let query_key = serde_json::to_string(query)?;
    let previous = read_section_occurrences(transaction, query, user_id)?;
    transaction.execute(
        &format!("DELETE FROM {table} WHERE user_id=?1 AND query=?2"),
        params![user_id.as_str(), query_key],
    )?;
    for (occurrence, incoming) in items.into_iter().enumerate() {
        let previous = previous.get(occurrence).filter(|previous| {
            previous.provider == incoming.fields().provider
                && previous.item_id == incoming.fields().item_id
        });
        let position = if incoming.fields().knowledge.position {
            FieldKnowledge::Known(incoming.fields().position)
        } else {
            previous
                .map(|previous| previous.position.clone())
                .unwrap_or_default()
        };
        match incoming {
            MediaItem::Album(album) => {
                write_album_observation(transaction, &album, user_id)?;
                write_section_occurrence(
                    transaction,
                    query,
                    user_id,
                    occurrence as i64,
                    &SectionOccurrence {
                        provider: album.media.provider,
                        item_id: album.media.item_id,
                        position,
                        context: TrackOccurrenceContext::default(),
                    },
                )?;
            }
            MediaItem::Track(track) => {
                let mut canonical_incoming = track.clone();
                if !track.knowledge.album {
                    canonical_incoming.knowledge.disc_number = false;
                    canonical_incoming.knowledge.track_number = false;
                    canonical_incoming.disc_number = 0;
                    canonical_incoming.track_number = 0;
                }
                write_track_observation(transaction, &canonical_incoming, user_id)?;
                let canonical = read_track(
                    transaction,
                    &track.media.provider,
                    &track.media.item_id,
                    user_id,
                )?
                .ok_or(CacheError::InvalidMediaObservation(
                    "canonical track is missing",
                ))?;
                let canonical_album =
                    if canonical.knowledge.album {
                        FieldKnowledge::Known(canonical.album.as_ref().map(|album| {
                            (album.media.provider.clone(), album.media.item_id.clone())
                        }))
                    } else {
                        FieldKnowledge::Unknown
                    };
                let mut context = TrackOccurrenceContext {
                    album: canonical_album,
                    disc_number: canonical
                        .knowledge
                        .disc_number
                        .then_some(canonical.disc_number),
                    track_number: canonical
                        .knowledge
                        .track_number
                        .then_some(canonical.track_number),
                    image: if canonical.knowledge.image {
                        FieldKnowledge::Known(canonical.image.clone())
                    } else {
                        FieldKnowledge::Unknown
                    },
                };
                if let Some(previous) = previous {
                    if !track.knowledge.album || previous.context.album == context.album {
                        context = previous.context.clone();
                    }
                }
                if track.knowledge.disc_number {
                    context.disc_number = Some(track.disc_number);
                }
                if track.knowledge.track_number {
                    context.track_number = Some(track.track_number);
                }
                if track.knowledge.image {
                    context.image = FieldKnowledge::Known(track.image.clone());
                }
                write_section_occurrence(
                    transaction,
                    query,
                    user_id,
                    occurrence as i64,
                    &SectionOccurrence {
                        provider: track.media.provider,
                        item_id: track.media.item_id,
                        position,
                        context,
                    },
                )?;
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

// Exact-context snapshot reads reconstruct only the selected occurrences.
pub(super) fn read_catalog_section(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<CatalogValue, CacheError> {
    let occurrences = read_section_occurrences(transaction, query, user_id)?;
    if matches!(query, CatalogQuery::ArtistAlbums { .. }) {
        let albums = occurrences
            .into_iter()
            .map(|occurrence| {
                let mut album = super::albums::read_album(
                    transaction,
                    &occurrence.provider,
                    &occurrence.item_id,
                    user_id,
                )?
                .ok_or(CacheError::InvalidMediaObservation(
                    "section album is missing",
                ))?;
                album.media.knowledge.position = !occurrence.position.is_unknown();
                album.media.position = match occurrence.position {
                    FieldKnowledge::Known(value) => value,
                    FieldKnowledge::Unknown => None,
                };
                Ok(album)
            })
            .collect::<Result<Vec<_>, CacheError>>()?;
        return Ok(CatalogValue::Albums(albums));
    }
    let tracks = occurrences
        .into_iter()
        .map(|occurrence| {
            let mut track = read_track(
                transaction,
                &occurrence.provider,
                &occurrence.item_id,
                user_id,
            )?
            .ok_or(CacheError::InvalidMediaObservation(
                "section track is missing",
            ))?;
            let canonical_album = if track.knowledge.album {
                FieldKnowledge::Known(
                    track
                        .album
                        .as_ref()
                        .map(|album| (album.media.provider.clone(), album.media.item_id.clone())),
                )
            } else {
                FieldKnowledge::Unknown
            };
            let has_same_album_selection = canonical_album == occurrence.context.album;
            track.knowledge.album = !occurrence.context.album.is_unknown();
            track.album = match occurrence.context.album {
                FieldKnowledge::Known(Some((provider, item_id))) => Some(
                    super::albums::read_album(transaction, &provider, &item_id, user_id)?.ok_or(
                        CacheError::InvalidMediaObservation("occurrence album is missing"),
                    )?,
                ),
                _ => None,
            };
            let album_thumb = track
                .album
                .as_ref()
                .and_then(|album| album.media.metadata.as_ref())
                .and_then(|metadata| {
                    metadata
                        .images
                        .iter()
                        .find(|image| image.image_type == "thumb")
                });
            let track_thumb = track.media.metadata.as_ref().and_then(|metadata| {
                metadata
                    .images
                    .iter()
                    .find(|image| image.image_type == "thumb")
            });
            let has_known_album_images = track
                .album
                .as_ref()
                .map(|album| {
                    album
                        .media
                        .metadata
                        .as_ref()
                        .map_or(album.media.knowledge.metadata, |metadata| {
                            metadata.knowledge.images
                        })
                })
                .unwrap_or(track.knowledge.album);
            let has_known_track_images = track
                .media
                .metadata
                .as_ref()
                .map_or(track.media.knowledge.metadata, |metadata| {
                    metadata.knowledge.images
                });
            if let Some(image) = album_thumb.or(track_thumb) {
                track.image = Some(image.clone());
                track.knowledge.image = true;
            } else if has_known_album_images && has_known_track_images {
                track.image = None;
                track.knowledge.image = true;
            } else if !has_same_album_selection {
                track.image = None;
                track.knowledge.image = false;
            }
            track.knowledge.disc_number = occurrence.context.disc_number.is_some();
            if super::track_reads::is_track_list_read(query) {
                track.knowledge.image = !occurrence.context.image.is_unknown();
                track.image = match occurrence.context.image {
                    FieldKnowledge::Known(image) => image,
                    FieldKnowledge::Unknown => None,
                };
            }
            track.disc_number = occurrence.context.disc_number.unwrap_or_default();
            track.knowledge.track_number = occurrence.context.track_number.is_some();
            track.track_number = occurrence.context.track_number.unwrap_or_default();
            track.media.knowledge.position = !occurrence.position.is_unknown();
            track.media.position = match occurrence.position {
                FieldKnowledge::Known(value) => value,
                FieldKnowledge::Unknown => None,
            };
            Ok(track)
        })
        .collect::<Result<Vec<_>, CacheError>>()?;
    Ok(
        if matches!(
            query,
            CatalogQuery::Search {
                media_type: MediaType::Track,
                ..
            }
        ) {
            CatalogValue::Search(MusicAssistantSearchResults {
                tracks,
                ..Default::default()
            })
        } else {
            CatalogValue::Tracks(tracks)
        },
    )
}

// Private occurrence codecs keep only contextual data at the result boundary.
fn section_table(query: &CatalogQuery) -> Result<&'static str, CacheError> {
    match query {
        CatalogQuery::ArtistAlbums { .. } => Ok("artist_album_read_items"),
        CatalogQuery::ArtistTracks { .. } | CatalogQuery::ArtistTopTracks { .. } => {
            Ok("artist_track_read_items")
        }
        CatalogQuery::AlbumTracks { .. } => Ok("album_track_read_items"),
        CatalogQuery::Search {
            media_type: MediaType::Track,
            ..
        }
        | CatalogQuery::TrackVersions { .. }
        | CatalogQuery::SimilarTracks { .. }
        | CatalogQuery::RecentlyPlayed { .. }
        | CatalogQuery::LibraryTracks { .. }
        | CatalogQuery::PlaylistTracks { .. } => Ok("track_list_items"),
        _ => Err(CacheError::InvalidMediaObservation(
            "query is not a catalog section",
        )),
    }
}

pub(super) fn section_occurrences_sql(query: &CatalogQuery) -> Result<String, CacheError> {
    let table = section_table(query)?;
    let context_columns = if matches!(query, CatalogQuery::ArtistAlbums { .. }) {
        "NULL,NULL,0,NULL,NULL"
    } else {
        "album_provider,album_item_id,album_known,disc_number,track_number"
    };
    let image_columns = if super::track_reads::is_track_list_read(query) {
        "image,image_known"
    } else {
        "NULL,0"
    };
    Ok(format!("SELECT provider,item_id,position,position_known,{context_columns},{image_columns} FROM {table} WHERE user_id=?1 AND query=?2 ORDER BY occurrence"))
}

fn read_section_occurrences(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Vec<SectionOccurrence>, CacheError> {
    let query_key = serde_json::to_string(query)?;
    let mut statement = transaction.prepare(&section_occurrences_sql(query)?)?;
    let occurrences = statement
        .query_map(params![user_id.as_str(), query_key], |row| {
            let album_provider: Option<String> = row.get(4)?;
            let album_item_id: Option<String> = row.get(5)?;
            let album =
                if row.get::<_, bool>(6)? {
                    FieldKnowledge::Known(album_provider.zip(album_item_id).map(
                        |(provider, item_id)| (ProviderInstanceId(provider), MediaId(item_id)),
                    ))
                } else {
                    FieldKnowledge::Unknown
                };
            Ok(SectionOccurrence {
                provider: ProviderInstanceId(row.get(0)?),
                item_id: MediaId(row.get(1)?),
                position: if row.get::<_, bool>(3)? {
                    FieldKnowledge::Known(row.get(2)?)
                } else {
                    FieldKnowledge::Unknown
                },
                context: TrackOccurrenceContext {
                    album,
                    disc_number: row.get(7)?,
                    track_number: row.get(8)?,
                    image: if row.get::<_, bool>(10)? {
                        FieldKnowledge::Known(
                            row.get::<_, Option<String>>(9)?
                                .map(|image| serde_json::from_str(&image))
                                .transpose()
                                .map_err(|error| {
                                    rusqlite::Error::FromSqlConversionFailure(
                                        9,
                                        rusqlite::types::Type::Text,
                                        Box::new(error),
                                    )
                                })?,
                        )
                    } else {
                        FieldKnowledge::Unknown
                    },
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(occurrences)
}

fn write_section_occurrence(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    occurrence: i64,
    reference: &SectionOccurrence,
) -> Result<(), CacheError> {
    let query_key = serde_json::to_string(query)?;
    let position = match reference.position {
        FieldKnowledge::Known(value) => value,
        FieldKnowledge::Unknown => None,
    };
    if matches!(query, CatalogQuery::ArtistAlbums { .. }) {
        transaction.execute("INSERT INTO artist_album_read_items(user_id,query,occurrence,provider,item_id,position,position_known) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![user_id.as_str(),query_key,occurrence,reference.provider.as_str(),reference.item_id.as_str(),position,!reference.position.is_unknown()])?;
    } else {
        let album = match &reference.context.album {
            FieldKnowledge::Known(Some(identity)) => Some(identity),
            _ => None,
        };
        transaction.execute(&format!("INSERT INTO {}(user_id,query,occurrence,provider,item_id,album_provider,album_item_id,album_known,disc_number,track_number,position,position_known)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)", section_table(query)?),
            params![user_id.as_str(),query_key,occurrence,reference.provider.as_str(),reference.item_id.as_str(),
                album.map(|(provider,_)|provider.as_str()),album.map(|(_,item_id)|item_id.as_str()),!reference.context.album.is_unknown(),
                reference.context.disc_number,reference.context.track_number,position,!reference.position.is_unknown()])?;
        if super::track_reads::is_track_list_read(query) {
            let image = match &reference.context.image {
                FieldKnowledge::Known(Some(image)) => Some(serde_json::to_string(image)?),
                _ => None,
            };
            transaction.execute("UPDATE track_list_items SET image=?4,image_known=?5 WHERE user_id=?1 AND query=?2 AND occurrence=?3",
                params![user_id.as_str(),query_key,occurrence,image,!reference.context.image.is_unknown()])?;
        }
    }
    Ok(())
}

// Schema replacement decodes old observations but never replays them into canonical media.

#[cfg(test)]
mod tests {
    use super::super::artist_reads::{read_artist_read, write_artist_read};
    use super::*;
    use crate::domain::identifiers::MediaId;

    // Ordered observations, summary knowledge and transaction failure.
    #[test]
    fn artist_section_references_derive_cover_from_current_occurrence_album_metadata() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        let user = UserId("user".into());
        let query = CatalogQuery::ArtistTopTracks {
            provider: "source".into(),
            item_id: MediaId("artist".into()),
        };
        let image =
            |path: &str| serde_json::json!({"type":"thumb","provider":"images","path":path});
        let mut tracks = Vec::new();
        for album in ["a", "b"] {
            tracks.push(serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"Track","image":image(album),
                "album":{"provider":"albums","item_id":album,"name":album,"metadata":{"images":[image(album)]}}})).unwrap());
        }
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(tracks),
            100,
        )
        .unwrap();
        let updated=serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"Track","image":image("c"),
            "album":{"provider":"albums","item_id":"c","name":"C","metadata":{"images":[image("c")]}}})).unwrap();
        write_track_observation(&transaction, &updated, &user).unwrap();
        let updated_album=serde_json::from_value(serde_json::json!({"provider":"albums","item_id":"a","name":"A","metadata":{"images":[image("a-new")]}})).unwrap();
        write_album_observation(&transaction, &updated_album, &user).unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let CatalogValue::Tracks(tracks) = read_artist_read(&transaction, &query, &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected tracks")
        };
        assert_eq!(tracks[0].image.as_ref().unwrap().path, "a-new");
        assert_eq!(tracks[1].image.as_ref().unwrap().path, "b");
        assert!(tracks.iter().all(|track| track.knowledge.image));
    }

    #[test]
    fn artist_section_references_keep_partial_context_separate_from_current_album_and_user() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        let user = UserId("user".into());
        let other_user = UserId("other".into());
        let first = CatalogQuery::ArtistTopTracks {
            provider: "source".into(),
            item_id: MediaId("first".into()),
        };
        let second = CatalogQuery::ArtistTracks {
            provider: "source".into(),
            item_id: MediaId("second".into()),
        };
        for (query, user, time, fields) in [
            (
                &first,
                &user,
                100,
                serde_json::json!({"favorite":true,"position":1,"album":{"provider":"a","item_id":"007","name":"A"},"disc_number":1,"track_number":4}),
            ),
            (
                &second,
                &user,
                200,
                serde_json::json!({"position":2,"album":{"provider":"b","item_id":"007","name":"B"},"disc_number":2,"track_number":9}),
            ),
            (&first, &user, 300, serde_json::json!({"disc_number":3})),
            (
                &first,
                &other_user,
                400,
                serde_json::json!({"favorite":false,"position":null,"album":null,"artists":[]}),
            ),
        ] {
            let mut value = serde_json::json!({"provider":"source","item_id":"007","name":"Track"});
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let transaction = connection.transaction().unwrap();
            write_artist_read(
                &transaction,
                query,
                user,
                &CatalogValue::Tracks(vec![serde_json::from_value(value).unwrap()]),
                time,
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        let transaction = connection.transaction().unwrap();
        let CatalogValue::Tracks(tracks) = read_artist_read(&transaction, &first, &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected tracks")
        };
        assert_eq!(
            tracks[0].album.as_ref().unwrap().media.provider.as_str(),
            "a"
        );
        assert_eq!((tracks[0].disc_number, tracks[0].track_number), (3, 4));
        assert_eq!(tracks[0].media.position, Some(1));
        assert_eq!(tracks[0].media.favorite, FieldKnowledge::Known(Some(true)));
        assert!(tracks[0].knowledge.artists && tracks[0].artists.is_empty());
        let CatalogValue::Tracks(tracks) = read_artist_read(&transaction, &second, &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected tracks")
        };
        assert_eq!(
            tracks[0].album.as_ref().unwrap().media.provider.as_str(),
            "b"
        );
        assert_eq!((tracks[0].disc_number, tracks[0].track_number), (2, 9));
        let numbers: (u16, u16) = transaction
            .query_row(
                "SELECT disc_number,track_number FROM album_tracks WHERE album_provider='b'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(numbers, (2, 9));
        let CatalogValue::Tracks(tracks) = read_artist_read(&transaction, &first, &other_user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected tracks")
        };
        assert!(tracks[0].knowledge.album && tracks[0].album.is_none());
        assert!(!tracks[0].knowledge.disc_number && !tracks[0].knowledge.track_number);
        assert!(tracks[0].media.knowledge.position && tracks[0].media.position.is_none());
        assert_eq!(tracks[0].media.favorite, FieldKnowledge::Known(Some(false)));
        let absent = CatalogQuery::ArtistTracks {
            provider: "source".into(),
            item_id: MediaId("absent".into()),
        };
        assert!(read_artist_read(&transaction, &absent, &user)
            .unwrap()
            .is_none());
    }

    #[test]
    fn artist_section_references_refresh_media_without_replacing_occurrence_album_context() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        let user = UserId("user".into());
        let query = CatalogQuery::ArtistTopTracks {
            provider: "source".into(),
            item_id: MediaId("artist".into()),
        };
        let mut items = Vec::new();
        for (album, disc, number) in [("a", 2, 3), ("b", 1, 8)] {
            items.push(serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"Old","duration":100,
                "metadata":{"description":"Old"},"album":{"provider":"album-source","item_id":album,"name":album},"disc_number":disc,"track_number":number})).unwrap());
        }
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(items),
            100,
        )
        .unwrap();
        transaction.commit().unwrap();
        let updated=serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"New","duration":200,
            "metadata":{"description":"New"},"album":{"provider":"album-source","item_id":"c","name":"C"},"disc_number":4,"track_number":2})).unwrap();
        let transaction = connection.transaction().unwrap();
        write_track_observation(&transaction, &updated, &user).unwrap();
        let query_key = serde_json::to_string(&query).unwrap();
        assert!(transaction.execute("INSERT INTO artist_track_read_items(user_id,query,occurrence,provider,item_id,album_known,position_known) VALUES(?1,?2,3,'missing','007',0,0)",params![user.as_str(),query_key]).is_err());
        assert!(transaction.execute("INSERT INTO artist_track_read_items(user_id,query,occurrence,provider,item_id,album_provider,album_item_id,album_known,position_known) VALUES(?1,?2,3,'source','007','missing','007',1,0)",params![user.as_str(),query_key]).is_err());
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let observed = read_artist_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.observed_at, 100);
        let CatalogValue::Tracks(tracks) = observed.value else {
            panic!("expected tracks")
        };
        assert_eq!(tracks.len(), 2);
        for (track, (album, disc, number)) in tracks.iter().zip([("a", 2, 3), ("b", 1, 8)]) {
            assert_eq!(track.media.name, "New");
            assert_eq!(track.duration, 200);
            assert_eq!(
                track
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .description
                    .as_deref(),
                Some("New")
            );
            assert_eq!(track.album.as_ref().unwrap().media.item_id.as_str(), album);
            assert_eq!((track.disc_number, track.track_number), (disc, number));
        }
    }

    #[test]
    fn track_canonical_section_summary_cannot_replay_stale_context_details() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        let user = UserId("user".into());
        let first = CatalogQuery::ArtistTopTracks {
            provider: "source".into(),
            item_id: MediaId("first".into()),
        };
        let second = CatalogQuery::ArtistTracks {
            provider: "source".into(),
            item_id: MediaId("second".into()),
        };
        for (query, time, fields) in [
            (
                &first,
                100,
                serde_json::json!({"duration":100,"metadata":{"description":"Old"},"artists":[{"provider":"artist-source","item_id":"a","name":"Artist","metadata":{"description":"Old artist"}}]}),
            ),
            (
                &second,
                200,
                serde_json::json!({"duration":200,"metadata":{"description":"New"},"artists":[{"provider":"artist-source","item_id":"a","name":"Artist","metadata":{"description":"New artist"}}]}),
            ),
            (&first, 300, serde_json::json!({})),
        ] {
            let mut value = serde_json::json!({"provider":"source","item_id":"007","name":"Track"});
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let transaction = connection.transaction().unwrap();
            write_artist_read(
                &transaction,
                query,
                &user,
                &CatalogValue::Tracks(vec![serde_json::from_value(value).unwrap()]),
                time,
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        let transaction = connection.transaction().unwrap();
        let track = read_track(
            &transaction,
            &crate::domain::identifiers::ProviderInstanceId("source".into()),
            &MediaId("007".into()),
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(track.duration, 200);
        assert_eq!(
            track
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("New")
        );
        assert_eq!(
            track.artists[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("New artist")
        );
        let CatalogValue::Tracks(tracks) = read_artist_read(&transaction, &first, &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected tracks")
        };
        assert_eq!(tracks[0].duration, 200);
    }

    #[test]
    fn album_canonical_section_summary_cannot_replay_stale_context_details() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        let user = UserId("user".into());
        let first = CatalogQuery::ArtistAlbums {
            provider: "source".into(),
            item_id: MediaId("first".into()),
        };
        let second = CatalogQuery::ArtistAlbums {
            provider: "source".into(),
            item_id: MediaId("second".into()),
        };
        for (query, time, fields) in [
            (
                &first,
                100,
                serde_json::json!({"year":2020,"metadata":{"description":"Old"}}),
            ),
            (
                &second,
                200,
                serde_json::json!({"year":2024,"metadata":{"description":"New"}}),
            ),
            (&first, 300, serde_json::json!({})),
        ] {
            let mut value = serde_json::json!({"provider":"source","item_id":"007","name":"Album"});
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let transaction = connection.transaction().unwrap();
            write_artist_read(
                &transaction,
                query,
                &user,
                &CatalogValue::Albums(vec![serde_json::from_value(value).unwrap()]),
                time,
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        let transaction = connection.transaction().unwrap();
        let album = super::super::albums::read_album(
            &transaction,
            &crate::domain::identifiers::ProviderInstanceId("source".into()),
            &MediaId("007".into()),
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(album.year, Some(2024));
        assert_eq!(
            album
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("New")
        );
        let CatalogValue::Albums(albums) = read_artist_read(&transaction, &first, &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected albums")
        };
        assert_eq!(albums[0].year, Some(2024));
    }

    #[test]
    fn artist_sections_round_trip_duplicates_partial_fields_and_atomic_failure() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        let user = UserId("user".into());
        let query = CatalogQuery::ArtistTopTracks {
            item_id: MediaId("artist".into()),
            provider: "source-a".into(),
        };
        let track=serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"007","name":"Track","duration":120,"metadata":{"description":"Keep"},"album":null,"artists":[{"provider":"source-a","item_id":"artist","name":"Artist"}]})).unwrap();
        let value = CatalogValue::Tracks(vec![
            track,
            serde_json::from_value(
                serde_json::json!({"provider":"source-b","item_id":"007","name":"Other"}),
            )
            .unwrap(),
        ]);
        let transaction = connection.transaction().unwrap();
        write_artist_read(&transaction, &query, &user, &value, 100).unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_artist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
        assert!(
            read_artist_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        transaction.commit().unwrap();
        let summary: crate::domain::media::Track = serde_json::from_value(
            serde_json::json!({"provider":"source-a","item_id":"007","name":"Renamed"}),
        )
        .unwrap();
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(vec![summary.clone(), summary]),
            200,
        )
        .unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let CatalogValue::Tracks(tracks) = read_artist_read(&transaction, &query, &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("expected tracks")
        };
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].duration, 120);
        assert_eq!(
            tracks[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Keep")
        );
        assert!(tracks[0].knowledge.album);
        assert!(!tracks[0].knowledge.image);
        transaction.commit().unwrap();
        let invalid=serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"bad","name":"Bad","artists":[{"provider":"source-a","item_id":"invalid","name":"Invalid","external_ids":[["discogs"]]}]})).unwrap();
        {
            let transaction = connection.transaction().unwrap();
            assert!(write_artist_read(
                &transaction,
                &query,
                &user,
                &CatalogValue::Tracks(vec![invalid]),
                300
            )
            .is_err());
        }
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_artist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            200
        );
        let count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM artist_track_read_items", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }
}
