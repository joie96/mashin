//! Pure radio identity and filtering rules preserve explicit seeds and provider aliases.

use super::favorites::has_same_media_identity;
use super::media::{MediaItem, Track};
use rand::seq::IndexedRandom;

pub(crate) struct RadioSelection {
    pub(crate) seed: MediaItem,
    pub(crate) started_tracks: Vec<Track>,
}

// Selection and filtering do not own a queue or perform I/O.
pub(crate) fn choose_seed(
    media: &[MediaItem],
    random: &mut impl rand::Rng,
) -> Result<MediaItem, String> {
    let seed = media
        .iter()
        .filter(|item| is_playable(item))
        .collect::<Vec<_>>()
        .choose(random)
        .map(|item| (*item).clone())
        .ok_or("radio requires at least one playable media item")?;
    if !matches!(
        seed,
        MediaItem::Track(_) | MediaItem::Album(_) | MediaItem::Artist(_) | MediaItem::Playlist(_)
    ) || media
        .iter()
        .any(|item| std::mem::discriminant(item) != std::mem::discriminant(&seed))
    {
        return Err("radio requires media items of the same supported type".into());
    }
    Ok(seed)
}

pub(crate) fn is_playable(item: &MediaItem) -> bool {
    item.fields().is_playable && item.fields().is_available()
}

pub(crate) fn media_uri(item: &MediaItem) -> Result<String, String> {
    let fields = item.fields();
    if let Some(uri) = fields
        .uri
        .as_ref()
        .map(|uri| uri.as_str())
        .filter(|uri| !uri.trim().is_empty())
    {
        return Ok(uri.to_owned());
    }
    let kind = match item {
        MediaItem::Track(_) => "track",
        MediaItem::Album(_) => "album",
        MediaItem::Artist(_) => "artist",
        MediaItem::Playlist(_) => "playlist",
        _ => return Err("unsupported radio seed type".into()),
    };
    if fields.provider.as_str().trim().is_empty() || fields.item_id.as_str().trim().is_empty() {
        return Err("radio seed has no Music Assistant URI".into());
    }
    Ok(format!(
        "{}://{kind}/{}",
        fields.provider.as_str(),
        fields.item_id.as_str()
    ))
}

pub(crate) fn filter_recommendations(tracks: Vec<Track>, started: &[Track]) -> Vec<Track> {
    let mut excluded = started
        .iter()
        .cloned()
        .map(MediaItem::Track)
        .collect::<Vec<_>>();
    let mut recommendations = Vec::new();
    for track in tracks {
        if !track.media.is_playable || !track.media.is_available() || track.duration > 20 * 60 {
            continue;
        }
        let candidate = MediaItem::Track(track.clone());
        if excluded.iter().any(|item| {
            has_same_media_identity(item, &candidate)
                || item.fields().uri.as_ref().is_some_and(|uri| {
                    !uri.as_str().trim().is_empty() && candidate.fields().uri.as_ref() == Some(uri)
                })
        }) {
            continue;
        }
        excluded.push(candidate);
        recommendations.push(track);
    }
    recommendations
}
