//! Application-lifetime recommendation order; no media payloads or database state are retained.
//! Position assignment shares one lock; sorting current media items runs after releasing it.

use crate::domain::identifiers::{MediaId, ProviderInstanceId};
use crate::domain::media::MediaType;
use crate::domain::recommendations::{RecommendationFolder, RecommendationView};
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, PartialEq, Eq, Hash)]
struct ItemOccurrence {
    media_type: MediaType,
    provider: ProviderInstanceId,
    item_id: MediaId,
    occurrence: usize,
}

#[derive(Default)]
struct FolderItemOrder {
    positions: HashMap<ItemOccurrence, usize>,
}

#[derive(Default)]
pub(crate) struct RecommendationItemOrder {
    folders: Mutex<HashMap<(RecommendationView, ProviderInstanceId, MediaId), FolderItemOrder>>,
}

impl RecommendationItemOrder {
    // Load-time projection keeps the original server inventory and occurrence metadata intact.
    pub(crate) fn order_folder(&self, view: RecommendationView, folder: &mut RecommendationFolder) {
        self.order_folder_with_random(view, folder, &mut rand::rng());
    }

    // Empty reads cannot freeze the first real selection; new items append without moving survivors.
    fn order_folder_with_random(
        &self,
        view: RecommendationView,
        folder: &mut RecommendationFolder,
        random: &mut impl rand::Rng,
    ) {
        if !view.should_shuffle_items(folder.item_id.as_str()) || folder.items.is_empty() {
            return;
        }
        let mut occurrences = HashMap::new();
        let items = std::mem::take(&mut folder.items)
            .into_iter()
            .map(|item| {
                let fields = item.fields();
                let identity = (
                    item.media_type(),
                    fields.provider.clone(),
                    fields.item_id.clone(),
                );
                let occurrence = occurrences.entry(identity.clone()).or_insert(0);
                let key = ItemOccurrence {
                    media_type: identity.0,
                    provider: identity.1,
                    item_id: identity.2,
                    occurrence: *occurrence,
                };
                *occurrence += 1;
                (key, item)
            })
            .collect::<Vec<_>>();
        let mut positioned_items = {
            let mut folders = self
                .folders
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let order = folders
                .entry((view, folder.provider.clone(), folder.item_id.clone()))
                .or_insert_with(|| {
                    let mut initial = items.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
                    initial.shuffle(random);
                    FolderItemOrder {
                        positions: initial
                            .into_iter()
                            .enumerate()
                            .map(|(index, key)| (key, index))
                            .collect(),
                    }
                });
            let mut next_position = order.positions.len();
            items
                .into_iter()
                .map(|(key, item)| {
                    let position = *order.positions.entry(key).or_insert_with(|| {
                        let position = next_position;
                        next_position += 1;
                        position
                    });
                    (position, item)
                })
                .collect::<Vec<_>>()
        };
        positioned_items.sort_by_key(|(position, _)| *position);
        folder.items = positioned_items.into_iter().map(|(_, item)| item).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};
    use std::sync::{Arc, Barrier};

    fn folder(item_id: &str, items: &[&str]) -> RecommendationFolder {
        serde_json::from_value(serde_json::json!({
            "provider":"source", "item_id":item_id, "name":"Section",
            "items":items.iter().map(|identity| serde_json::json!({
                "media_type":"track", "provider":"source", "item_id":identity, "name":identity,
            })).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    fn identities(folder: &RecommendationFolder) -> Vec<String> {
        folder
            .items
            .iter()
            .map(|item| item.fields().item_id.as_str().to_owned())
            .collect()
    }

    #[test]
    fn recommendation_order_shuffles_once_and_survives_reversed_reads() {
        let ordering = RecommendationItemOrder::default();
        let input = ["0", "1", "2", "3", "4", "5", "6", "7"];
        let mut initial = folder("user_recommendations", &input);
        let mut expected = input.to_vec();
        expected.shuffle(&mut StdRng::seed_from_u64(42));
        ordering.order_folder_with_random(
            RecommendationView::Home,
            &mut initial,
            &mut StdRng::seed_from_u64(42),
        );
        assert_eq!(identities(&initial), expected);
        assert_ne!(identities(&initial), input);
        let mut refreshed = folder(
            "user_recommendations",
            &["7", "6", "5", "4", "3", "2", "1", "0"],
        );
        ordering.order_folder_with_random(
            RecommendationView::Home,
            &mut refreshed,
            &mut StdRng::seed_from_u64(77),
        );
        assert_eq!(identities(&refreshed), identities(&initial));
        let restarted = RecommendationItemOrder::default();
        restarted.order_folder_with_random(
            RecommendationView::Home,
            &mut refreshed,
            &mut StdRng::seed_from_u64(77),
        );
        assert_ne!(identities(&refreshed), identities(&initial));
    }

    #[test]
    fn recommendation_order_preserves_metadata_and_appends_new_items_after_survivors() {
        let ordering = RecommendationItemOrder::default();
        let mut initial = folder("global_genre_albums_rock", &["a", "b", "c", "d"]);
        ordering.order_folder(RecommendationView::Explore, &mut initial);
        let mut refreshed = folder("global_genre_albums_rock", &["new", "d", "c", "b", "a"]);
        for item in &mut refreshed.items {
            item.fields_mut().name = "Updated".into();
        }
        ordering.order_folder(RecommendationView::Explore, &mut refreshed);
        let mut expected = identities(&initial);
        expected.push("new".into());
        assert_eq!(identities(&refreshed), expected);
        assert!(refreshed
            .items
            .iter()
            .all(|item| item.fields().name == "Updated"));
        refreshed
            .items
            .retain(|item| item.fields().item_id.as_str() != "b");
        ordering.order_folder(RecommendationView::Explore, &mut refreshed);
        assert_eq!(
            identities(&refreshed),
            expected
                .into_iter()
                .filter(|identity| identity != "b")
                .collect::<Vec<_>>()
        );
        let mut restored = folder("global_genre_albums_rock", &["d", "c", "b", "a"]);
        ordering.order_folder(RecommendationView::Explore, &mut restored);
        assert_eq!(identities(&restored), identities(&initial));
    }

    #[test]
    fn recommendation_order_concurrent_reads_preserve_positions_and_current_metadata() {
        let ordering = Arc::new(RecommendationItemOrder::default());
        let mut initial = folder("user_recommendations", &["a", "b", "c", "d"]);
        ordering.order_folder_with_random(
            RecommendationView::Home,
            &mut initial,
            &mut StdRng::seed_from_u64(42),
        );
        let mut expected = identities(&initial);
        expected.push("new".into());
        let reader_count = 8;
        let barrier = Arc::new(Barrier::new(reader_count));
        let readers = (0..reader_count)
            .map(|reader_index| {
                let ordering = Arc::clone(&ordering);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut current = folder("user_recommendations", &["d", "c", "b", "a", "new"]);
                    let item_count = current.items.len();
                    current.items.rotate_left(reader_index % item_count);
                    let name = format!("Reader {reader_index}");
                    for item in &mut current.items {
                        item.fields_mut().name = name.clone();
                    }
                    barrier.wait();
                    ordering.order_folder(RecommendationView::Home, &mut current);
                    (current, name)
                })
            })
            .collect::<Vec<_>>();
        for reader in readers {
            let (current, name) = reader.join().unwrap();
            assert_eq!(identities(&current), expected);
            assert!(current.items.iter().all(|item| item.fields().name == name));
        }
    }

    #[test]
    fn recommendation_order_keeps_duplicates_and_exact_provider_and_type_identities() {
        let ordering = RecommendationItemOrder::default();
        let mut initial = folder("user_recommendations", &["a", "a", "b"]);
        initial.items[1].fields_mut().provider = ProviderInstanceId("other".into());
        let mut duplicate = initial.items[0].clone();
        duplicate.fields_mut().name = "Second occurrence".into();
        initial.items.push(duplicate);
        initial.items.push(
            serde_json::from_value(serde_json::json!({
                "media_type":"album", "provider":"source", "item_id":"a", "name":"Album",
            }))
            .unwrap(),
        );
        let source = initial.clone();
        ordering.order_folder(RecommendationView::Home, &mut initial);
        let mut refreshed = source;
        ordering.order_folder(RecommendationView::Home, &mut refreshed);
        assert_eq!(refreshed.items, initial.items);
        assert_eq!(refreshed.items.len(), 5);
    }

    #[test]
    fn recommendation_order_leaves_charts_untouched_and_ignores_empty_reads() {
        let ordering = RecommendationItemOrder::default();
        let mut empty = folder("user_recommendations", &[]);
        ordering.order_folder(RecommendationView::Home, &mut empty);
        assert!(ordering.folders.lock().unwrap().is_empty());
        let mut charts = folder("global_top_tracks", &["c", "a", "b"]);
        let original = charts.clone();
        ordering.order_folder(RecommendationView::Explore, &mut charts);
        assert_eq!(charts, original);
        assert!(ordering.folders.lock().unwrap().is_empty());
    }
}
