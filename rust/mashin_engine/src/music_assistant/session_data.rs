//! Orders session startup across independent recommendation and user-data loaders.
//! Publication stays synchronous; transport awaits never hold application locks.

use super::client::MusicAssistantClient;
use super::user_data::InitialUserDataUpdate;

pub(crate) enum InitialSessionDataUpdate {
    RecommendationData,
    UserData(InitialUserDataUpdate),
}

pub(crate) struct InitialSessionDataResult {
    pub(crate) recommendation_data: Result<(), String>,
    pub(crate) user_data: Result<(), String>,
}

impl InitialSessionDataResult {
    // User-data failure takes precedence without losing either component's result.
    pub(crate) fn into_result(self) -> Result<(), String> {
        self.user_data.and(self.recommendation_data)
    }
}

impl MusicAssistantClient {
    // Session startup is ordered independently of page subscriptions.
    pub(crate) async fn load_initial_session_data(
        &self,
        mut publish: impl FnMut(InitialSessionDataUpdate) -> Result<(), String>,
    ) -> InitialSessionDataResult {
        let mut publication_error = None;
        let recommendation_data = self
            .load_recommendation_data(|| {
                if let Err(error) = publish(InitialSessionDataUpdate::RecommendationData) {
                    publication_error = Some(error);
                }
            })
            .await;
        if let Some(error) = publication_error {
            return InitialSessionDataResult {
                recommendation_data,
                user_data: Err(error),
            };
        }
        let user_data = self
            .load_initial_user_data(|update| publish(InitialSessionDataUpdate::UserData(update)))
            .await;
        InitialSessionDataResult {
            recommendation_data,
            user_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_assistant::client::tests::server;
    use crate::music_assistant::models::identifiers::UserId;
    use serde_json::json;

    #[tokio::test]
    async fn initial_session_data_loads_recommendations_before_user_data() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([
                    {"item_id":"user_exploration_weekly", "provider":"lb"},
                    {"item_id":"user_recommendations", "provider":"lb"}
                ]),
            ),
            ("music/recommendations/items", json!([])),
            ("music/recommendations/items", json!([])),
            (
                "auth/me",
                json!({"user_id":"user-one", "username":"test", "role":"user"}),
            ),
            ("music/tracks/library_items", json!([])),
            ("music/albums/library_items", json!([])),
            ("music/artists/library_items", json!([])),
            ("music/playlists/library_items", json!([])),
            (
                "music/playlists/library_items",
                json!([{
                    "item_id":"42", "provider":"library", "name":"Private",
                    "access":{"owner":"user-one", "sharing":"private"}
                }]),
            ),
            ("music/playlists/playlist_tracks", json!([])),
        ]);
        let mut phases = Vec::new();
        client
            .load_initial_session_data(|update| {
                match update {
                    InitialSessionDataUpdate::RecommendationData => {}
                    InitialSessionDataUpdate::UserData(update) => match update {
                        InitialUserDataUpdate::UserIdentity(_) => phases.push("identity"),
                        InitialUserDataUpdate::Favorites(_) => phases.push("favorites"),
                        InitialUserDataUpdate::Playlists(playlists) => {
                            assert!(playlists[0].is_loading_tracks);
                            phases.push("metadata");
                        }
                        InitialUserDataUpdate::Playlist(playlist) => {
                            assert!(!playlist.is_loading_tracks);
                            phases.push("tracks");
                        }
                    },
                }
                Ok(())
            })
            .await
            .into_result()
            .unwrap();
        assert_eq!(phases, ["identity", "favorites", "metadata", "tracks"]);
        let requests = worker.join().unwrap();
        assert_eq!(requests[1]["args"]["item_id"], "user_recommendations");
        assert_eq!(requests[2]["args"]["item_id"], "user_exploration_weekly");
        assert_eq!(requests[7]["args"]["favorite"], true);
        assert_eq!(requests[8]["args"]["summary"], true);
        assert!(requests[8]["args"].get("provider").is_none());
    }

    #[tokio::test]
    async fn initial_session_data_publishes_user_data_after_recommendation_failure() {
        let (client, worker) = server(vec![
            ("music/recommendations", json!({"test_http_status":500})),
            ("music/tracks/library_items", json!([])),
            ("music/albums/library_items", json!([])),
            ("music/artists/library_items", json!([])),
            ("music/playlists/library_items", json!([])),
            (
                "music/playlists/library_items",
                json!([{
                    "item_id":"42", "provider":"library", "name":"Private",
                    "access":{"owner":"user-one", "sharing":"private"}
                }]),
            ),
            ("music/playlists/playlist_tracks", json!([])),
            ("music/recommendations", json!([])),
        ]);
        client
            .initialize_current_user_id(UserId("user-one".into()))
            .unwrap();
        let mut phases = Vec::new();
        let result = client
            .load_initial_session_data(|update| {
                match update {
                    InitialSessionDataUpdate::UserData(InitialUserDataUpdate::Favorites(_)) => {
                        phases.push("favorites")
                    }
                    InitialSessionDataUpdate::UserData(InitialUserDataUpdate::Playlists(_)) => {
                        phases.push("metadata")
                    }
                    InitialSessionDataUpdate::UserData(InitialUserDataUpdate::Playlist(
                        playlist,
                    )) => {
                        assert!(!playlist.is_loading_tracks);
                        phases.push("tracks");
                    }
                    _ => {}
                }
                Ok(())
            })
            .await;
        assert!(result.recommendation_data.is_err());
        assert!(result.user_data.is_ok());
        assert_eq!(phases, ["favorites", "metadata", "tracks"]);
        client.load_recommendation_data(|| {}).await.unwrap();
        assert_eq!(worker.join().unwrap().len(), 8);
    }

    #[tokio::test]
    async fn initial_session_data_retains_recommendation_success_after_user_data_failure() {
        let (client, worker) = server(vec![
            ("music/recommendations", json!([])),
            (
                "music/tracks/library_items",
                json!({"test_http_status":500}),
            ),
        ]);
        client
            .initialize_current_user_id(UserId("user-one".into()))
            .unwrap();
        let result = client.load_initial_session_data(|_| Ok(())).await;
        assert!(result.recommendation_data.is_ok());
        assert!(result.user_data.is_err());
        assert!(result.into_result().is_err());
        client
            .load_recommendation_data(|| panic!("successful recommendations must remain cached"))
            .await
            .unwrap();
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn initial_session_data_stops_after_publication_failure() {
        let (client, worker) = server(vec![("music/recommendations", json!([]))]);
        let error = client
            .load_initial_session_data(|_| Err("session replaced".into()))
            .await
            .into_result()
            .unwrap_err();
        assert_eq!(error, "session replaced");
        assert_eq!(worker.join().unwrap().len(), 1);
    }
}
