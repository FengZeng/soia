use super::playlist_service::PreparedPlaylist;
use crate::protocol::PlaylistSourceContinuationResultDto;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OPERATION_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PENDING_OPERATIONS: usize = 32;
const MAX_COMPLETED_OPERATIONS: usize = 64;

struct PendingOperation {
    client_id: String,
    prepared_playlist: PreparedPlaylist,
    playback_key: String,
    playback_title: Option<String>,
    created_at: Instant,
}

struct CompletedOperation {
    client_id: String,
    completed_at: Instant,
    outcome: Result<PlaylistSourceContinuationResultDto, String>,
}

#[derive(Default)]
struct OperationState {
    pending: HashMap<String, PendingOperation>,
    pending_order: VecDeque<String>,
    in_flight: std::collections::HashSet<String>,
    completed: HashMap<String, CompletedOperation>,
    completed_order: VecDeque<String>,
}

/// Keeps trusted prepared playlist entries in Core between a client-local confirmation dialog and
/// its continuation. Neither pending entries nor operation IDs are shared across clients.
pub(crate) struct PlaylistSourceOperationStore {
    state: Mutex<OperationState>,
}

pub(crate) enum PlaylistSourceOperationClaim {
    Execute {
        prepared_playlist: PreparedPlaylist,
        playback_key: String,
        playback_title: Option<String>,
    },
    Completed(Result<PlaylistSourceContinuationResultDto, String>),
}

impl PlaylistSourceOperationStore {
    pub(crate) fn new() -> Self {
        Self { state: Mutex::new(OperationState::default()) }
    }

    pub(crate) fn begin(
        &self,
        client_id: String,
        prepared_playlist: PreparedPlaylist,
        playback_key: String,
        playback_title: Option<String>,
    ) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        purge_expired(&mut state);
        while state.pending_order.len() >= MAX_PENDING_OPERATIONS {
            if let Some(id) = state.pending_order.pop_front() {
                state.pending.remove(&id);
            }
        }
        let operation_id = uuid::Uuid::now_v7().to_string();
        state.pending.insert(operation_id.clone(), PendingOperation {
            client_id,
            prepared_playlist,
            playback_key,
            playback_title,
            created_at: Instant::now(),
        });
        state.pending_order.push_back(operation_id.clone());
        Ok(operation_id)
    }

    pub(crate) fn claim(
        &self,
        client_id: &str,
        operation_id: &str,
    ) -> Result<PlaylistSourceOperationClaim, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        purge_expired(&mut state);
        if let Some(completed) = state.completed.get(operation_id) {
            if completed.client_id == client_id {
                return Ok(PlaylistSourceOperationClaim::Completed(completed.outcome.clone()));
            }
            return Err("playlist source operation belongs to another client".to_string());
        }
        let operation_client_id = state
            .pending
            .get(operation_id)
            .map(|operation| operation.client_id.as_str())
            .ok_or_else(|| "playlist source operation expired or was already completed".to_string())?;
        if operation_client_id != client_id {
            return Err("playlist source operation belongs to another client".to_string());
        }
        if !state.in_flight.insert(operation_id.to_string()) {
            return Err("playlist source continuation is already in progress".to_string());
        }
        let operation = state
            .pending
            .get(operation_id)
            .expect("checked pending playlist source operation");
        Ok(PlaylistSourceOperationClaim::Execute {
            prepared_playlist: operation.prepared_playlist.clone(),
            playback_key: operation.playback_key.clone(),
            playback_title: operation.playback_title.clone(),
        })
    }

    pub(crate) fn release(
        &self,
        client_id: &str,
        operation_id: &str,
    ) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        purge_expired(&mut state);
        let operation = state
            .pending
            .get(operation_id)
            .ok_or_else(|| "playlist source operation expired or was already completed".to_string())?;
        if operation.client_id != client_id {
            return Err("playlist source operation belongs to another client".to_string());
        }
        state.in_flight.remove(operation_id);
        Ok(())
    }

    pub(crate) fn complete(
        &self,
        client_id: String,
        operation_id: String,
        outcome: Result<PlaylistSourceContinuationResultDto, String>,
    ) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        purge_expired(&mut state);
        while state.completed_order.len() >= MAX_COMPLETED_OPERATIONS {
            if let Some(id) = state.completed_order.pop_front() {
                state.completed.remove(&id);
            }
        }
        let pending = state.pending.remove(&operation_id);
        if pending.as_ref().is_some_and(|operation| operation.client_id != client_id) {
            return Err("playlist source operation belongs to another client".to_string());
        }
        state.pending_order.retain(|id| id != &operation_id);
        state.in_flight.remove(&operation_id);
        state.completed.insert(operation_id.clone(), CompletedOperation {
            client_id,
            completed_at: Instant::now(),
            outcome,
        });
        state.completed_order.push_back(operation_id);
        Ok(())
    }
}

fn purge_expired(state: &mut OperationState) {
    let now = Instant::now();
    state.pending.retain(|_, operation| now.duration_since(operation.created_at) < OPERATION_TTL);
    state.pending_order.retain(|id| state.pending.contains_key(id));
    state.in_flight.retain(|id| state.pending.contains_key(id));
    state.completed.retain(|_, operation| now.duration_since(operation.completed_at) < OPERATION_TTL);
    state.completed_order.retain(|id| state.completed.contains_key(id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::playlist_service::PreparedPlaylistEntry;

    fn prepared() -> PreparedPlaylist {
        PreparedPlaylist {
            name: "Test playlist".to_string(),
            entries: vec![PreparedPlaylistEntry {
                path: "https://example.test/video.mp4".to_string(),
                ..Default::default()
            }],
        }
    }

    fn result() -> PlaylistSourceContinuationResultDto {
        PlaylistSourceContinuationResultDto {
            playlist_id: Some("playlist-1".to_string()),
            playback_key: Some("https://example.test/video.mp4".to_string()),
            title: None,
            is_live_playback: false,
            superseded: false,
        }
    }

    #[test]
    fn only_the_originating_client_can_take_an_operation() {
        let store = PlaylistSourceOperationStore::new();
        let operation_id = store
            .begin("desktop-a".to_string(), prepared(), "https://example.test/video.mp4".to_string(), None)
            .expect("begin operation");

        assert!(store.claim("desktop-b", &operation_id).is_err());
        assert!(matches!(store.claim("desktop-a", &operation_id).expect("claim operation"), PlaylistSourceOperationClaim::Execute { .. }));
    }

    #[test]
    fn completed_operation_returns_its_cached_result_to_the_same_client() {
        let store = PlaylistSourceOperationStore::new();
        let operation_id = store
            .begin("desktop-a".to_string(), prepared(), "https://example.test/video.mp4".to_string(), None)
            .expect("begin operation");
        let _ = store.claim("desktop-a", &operation_id).expect("claim operation");
        store.complete("desktop-a".to_string(), operation_id.clone(), Ok(result()))
            .expect("complete operation");

        let PlaylistSourceOperationClaim::Completed(Ok(replayed)) = store
            .claim("desktop-a", &operation_id)
            .expect("lookup result") else { panic!("cached result"); };
        assert_eq!(replayed.playlist_id.as_deref(), Some("playlist-1"));
        assert_eq!(replayed.playback_key.as_deref(), Some("https://example.test/video.mp4"));
        assert!(store.claim("desktop-b", &operation_id).is_err());
    }

    #[test]
    fn completed_failure_is_replayed_without_claiming_the_operation_again() {
        let store = PlaylistSourceOperationStore::new();
        let operation_id = store
            .begin("desktop-a".to_string(), prepared(), "https://example.test/video.mp4".to_string(), None)
            .expect("begin operation");
        let _ = store.claim("desktop-a", &operation_id).expect("claim operation");
        store.complete(
            "desktop-a".to_string(),
            operation_id.clone(),
            Err("source loading failed".to_string()),
        )
        .expect("complete operation");

        let PlaylistSourceOperationClaim::Completed(Err(error)) = store
            .claim("desktop-a", &operation_id)
            .expect("replay failure") else { panic!("cached failure"); };
        assert_eq!(error, "source loading failed");
    }
}
