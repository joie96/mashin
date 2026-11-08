//! Pure favorite intent and provider/library alias rules; no tasks or confirmed store.

use super::library::{Favorites, UserDataSnapshot};
use super::media::MediaItem;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FavoriteOverride {
    pub(crate) item: MediaItem,
    pub(crate) is_favorite: bool,
}

#[derive(Debug, Clone)]
struct FavoriteIntent {
    item: MediaItem,
    is_favorite: bool,
    is_uncertain: bool,
    has_inflight_write: bool,
    revision: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FavoriteWork {
    Write { item: MediaItem, is_favorite: bool },
    WaitForAliases,
    Finished,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FavoriteIntents {
    pending: HashMap<String, FavoriteIntent>,
    retired_workers: HashMap<String, String>,
    revision: u64,
}

impl FavoriteIntents {
    // A canonical readback changes the item, never the identity of its active worker.
    pub(crate) fn submit(
        &mut self,
        confirmed: &Favorites,
        item: MediaItem,
        is_favorite: bool,
    ) -> (String, MediaItem, bool) {
        self.revision = self.revision.saturating_add(1);
        let pending = self
            .pending
            .iter_mut()
            .find(|(_, pending)| has_same_media_identity(&pending.item, &item));
        if let Some((identity, pending)) = pending {
            pending.is_favorite = is_favorite;
            pending.revision = self.revision;
            let should_start = pending.is_uncertain && !pending.has_inflight_write;
            pending.is_uncertain = false;
            return (identity.clone(), pending.item.clone(), should_start);
        }
        let canonical = confirmed
            .items()
            .into_iter()
            .find(|candidate| has_same_media_identity(candidate, &item))
            .unwrap_or(item);
        let identity = uuid::Uuid::new_v4().to_string();
        self.pending.insert(
            identity.clone(),
            FavoriteIntent {
                item: canonical.clone(),
                is_favorite,
                is_uncertain: false,
                has_inflight_write: false,
                revision: self.revision,
            },
        );
        (identity, canonical, true)
    }

    pub(crate) fn finish(
        &mut self,
        identity: &str,
        written_state: bool,
        confirmed: Option<&MediaItem>,
    ) -> bool {
        if let Some(survivor) = self.retired_workers.remove(identity) {
            if let (Some(pending), Some(confirmed)) = (self.pending.get_mut(&survivor), confirmed) {
                pending.item = confirmed.clone();
            }
            return true;
        }
        if let Some(confirmed) = confirmed {
            let aliases = self
                .pending
                .iter()
                .filter(|(other_identity, other)| {
                    other_identity.as_str() != identity
                        && has_same_media_identity(&other.item, confirmed)
                })
                .map(|(identity, _)| identity.clone())
                .collect::<Vec<_>>();
            for alias in aliases {
                let Some(other) = self.pending.remove(&alias) else {
                    continue;
                };
                if let Some(pending) = self.pending.get_mut(identity) {
                    if other.revision > pending.revision {
                        pending.is_favorite = other.is_favorite;
                        pending.revision = other.revision;
                    }
                    if other.has_inflight_write {
                        self.retired_workers
                            .insert(alias.clone(), identity.to_owned());
                    }
                    for survivor in self.retired_workers.values_mut() {
                        if *survivor == alias {
                            *survivor = identity.to_owned();
                        }
                    }
                }
            }
        }
        let Some(pending) = self.pending.get_mut(identity) else {
            return true;
        };
        pending.has_inflight_write = false;
        if pending.is_uncertain {
            return true;
        }
        if let Some(confirmed) = confirmed {
            pending.item = confirmed.clone();
        }
        if pending.is_favorite != written_state
            || self
                .retired_workers
                .values()
                .any(|survivor| survivor == identity)
        {
            return false;
        }
        self.pending.remove(identity);
        true
    }

    // Reserve the write under the application lock before network work can begin.
    pub(crate) fn begin_work(&mut self, identity: &str) -> FavoriteWork {
        let work = self.work(identity);
        if matches!(work, FavoriteWork::Write { .. }) {
            if let Some(pending) = self.pending.get_mut(identity) {
                pending.has_inflight_write = true;
            }
        }
        work
    }

    // Removal retires aliases and their in-flight workers without inventing a new rating.
    pub(crate) fn retire_removed_item(&mut self, item: &MediaItem) {
        self.revision = self.revision.saturating_add(1);
        let retired = self
            .pending
            .iter()
            .filter(|(_, pending)| has_same_media_identity(&pending.item, item))
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>();
        self.pending
            .retain(|identity, _| !retired.contains(identity));
        self.retired_workers
            .retain(|worker, survivor| !retired.contains(worker) && !retired.contains(survivor));
    }

    // Queries and read projections never write into the confirmed input.
    pub(crate) fn has_active_worker(&self, identity: &str) -> bool {
        self.pending.contains_key(identity) || self.retired_workers.contains_key(identity)
    }

    pub(crate) fn work(&self, identity: &str) -> FavoriteWork {
        let Some(pending) = self
            .pending
            .get(identity)
            .filter(|pending| !pending.is_uncertain)
        else {
            return FavoriteWork::Finished;
        };
        if self
            .retired_workers
            .values()
            .any(|survivor| survivor == identity)
        {
            return FavoriteWork::WaitForAliases;
        }
        FavoriteWork::Write {
            item: pending.item.clone(),
            is_favorite: pending.is_favorite,
        }
    }

    pub(crate) fn desired_state(&self, identity: &str) -> Option<bool> {
        self.pending
            .get(identity)
            .filter(|pending| !pending.is_uncertain)
            .map(|pending| pending.is_favorite)
    }

    // Unknown write outcomes stay distinct from both server confirmation and the user's wish.
    pub(crate) fn mark_uncertain(&mut self, identity: &str) {
        if let Some(survivor) = self.retired_workers.remove(identity) {
            self.revision = self.revision.saturating_add(1);
            if let Some(pending) = self.pending.get_mut(&survivor) {
                pending.is_uncertain = true;
                pending.revision = self.revision;
            }
            return;
        }
        self.revision = self.revision.saturating_add(1);
        if let Some(pending) = self.pending.get_mut(identity) {
            pending.is_uncertain = true;
            pending.has_inflight_write = false;
            pending.revision = self.revision;
        }
    }

    // A complete read started after uncertainty resolves it without repeating the mutation.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn reconcile_complete_read(&mut self, started_revision: u64) {
        self.pending.retain(|identity, pending| {
            !pending.is_uncertain
                || pending.revision > started_revision
                || self
                    .retired_workers
                    .values()
                    .any(|survivor| survivor == identity)
        });
    }

    pub(crate) fn project(&self, confirmed: &UserDataSnapshot) -> UserDataSnapshot {
        let mut projection = confirmed.clone();
        for pending in self.pending.values() {
            projection.record_favorite_state(pending.item.clone(), pending.is_favorite);
        }
        projection
    }
}

// Aliases match only within one media type; provider instances remain distinct.
pub(crate) fn has_same_media_identity(left: &MediaItem, right: &MediaItem) -> bool {
    if std::mem::discriminant(left) != std::mem::discriminant(right) {
        return false;
    }
    let left = left.fields();
    let right = right.fields();
    (left.provider == right.provider && left.item_id == right.item_id)
        || left.provider_mappings.iter().any(|mapping| {
            (mapping.provider_instance == right.provider && mapping.item_id == right.item_id)
                || right.provider_mappings.iter().any(|other| {
                    mapping.provider_instance == other.provider_instance
                        && mapping.item_id == other.item_id
                })
        })
        || right.provider_mappings.iter().any(|mapping| {
            mapping.provider_instance == left.provider && mapping.item_id == left.item_id
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_removal_retires_aliased_workers_and_preserves_other_media_types() {
        let removed: MediaItem = serde_json::from_value(serde_json::json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Removed",
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote"}]})).unwrap();
        let alias: MediaItem = serde_json::from_value(serde_json::json!({"media_type":"playlist","provider":"source-a","item_id":"remote","name":"Alias"})).unwrap();
        let mut unlinked = removed.clone();
        unlinked.fields_mut().provider_mappings.clear();
        let mut intents = FavoriteIntents::default();
        let snapshot = UserDataSnapshot::default();
        let (alias_worker, _, _) = intents.submit(&snapshot.favorites, alias, true);
        intents.begin_work(&alias_worker);
        let (library_worker, _, _) = intents.submit(&snapshot.favorites, unlinked, true);
        intents.begin_work(&library_worker);
        assert!(!intents.finish(&library_worker, true, Some(&removed)));
        let unrelated = track("library", "42", serde_json::json!([]));
        let (other_worker, _, _) = intents.submit(&snapshot.favorites, unrelated, true);
        let revision = intents.revision();
        intents.retire_removed_item(&removed);
        assert!(intents.revision() > revision);
        for identity in [&alias_worker, &library_worker] {
            assert!(!intents.has_active_worker(identity));
            assert_eq!(intents.work(identity), FavoriteWork::Finished);
        }
        assert!(intents.has_active_worker(&other_worker));
        assert!(intents.finish(&alias_worker, true, Some(&removed)));
        let projection = intents.project(&snapshot);
        assert!(projection.favorites.playlists.is_empty());
        assert_eq!(projection.favorites.tracks.len(), 1);
    }

    fn track(provider: &str, item_id: &str, mappings: serde_json::Value) -> MediaItem {
        MediaItem::Track(
            serde_json::from_value(serde_json::json!({
                "item_id":item_id, "provider":provider, "name":"Track", "provider_mappings":mappings
            }))
            .unwrap(),
        )
    }

    #[test]
    fn pending_projection_rebases_without_changing_confirmed_favorites() {
        let mut intents = FavoriteIntents::default();
        let original = UserDataSnapshot::default();
        let item = track("source-one", "source-id", serde_json::json!([]));
        let (identity, _, should_start) = intents.submit(&original.favorites, item.clone(), true);
        assert!(should_start);
        assert_eq!(intents.project(&original).favorites.tracks.len(), 1);
        assert!(original.favorites.tracks.is_empty());
        let mut refreshed = original.clone();
        refreshed.favorites.set_item(item.clone(), true);
        let (same_identity, _, should_start) = intents.submit(&refreshed.favorites, item, false);
        assert_eq!(identity, same_identity);
        assert!(!should_start);
        assert!(intents.project(&refreshed).favorites.tracks.is_empty());
        assert_eq!(refreshed.favorites.tracks.len(), 1);
        assert!(!intents.finish(&identity, true, None));
        assert_eq!(intents.desired_state(&identity), Some(false));
        assert!(intents.finish(&identity, false, None));
        assert_eq!(intents.project(&refreshed), refreshed);
    }

    #[test]
    fn canonical_readback_keeps_the_worker_key_for_followup_actions() {
        let mut intents = FavoriteIntents::default();
        let favorites = Favorites::default();
        let item = track("source-one", "source-id", serde_json::json!([]));
        let canonical = track(
            "library",
            "42",
            serde_json::json!([
                {"item_id":"source-id", "provider_instance":"source-one", "provider_domain":"test"}
            ]),
        );
        let (identity, _, _) = intents.submit(&favorites, item.clone(), true);
        intents.submit(&favorites, item, false);
        assert!(!intents.finish(&identity, true, Some(&canonical)));
        let (same_identity, current, should_start) =
            intents.submit(&favorites, canonical.clone(), true);
        assert_eq!(same_identity, identity);
        assert_eq!(current, canonical);
        assert!(!should_start);
        assert!(intents.finish(&identity, true, Some(&canonical)));
    }
}
