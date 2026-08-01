use crate::protocol::{MediaTrackDto, PlaybackSnapshotDto, PROTOCOL_VERSION};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Internal Core state. It intentionally remains separate from the protocol DTO so
/// implementation details can evolve without becoming client contract.
#[derive(Clone)]
pub(crate) struct PlaybackSnapshot {
    pub playback_session_id: Option<String>,
    pub playback_key: Option<String>,
    pub playback_playlist_id: Option<String>,
    pub title: Option<String>,
    pub duration: f64,
    pub position: f64,
    pub buffered_position: f64,
    pub is_playing: bool,
    pub is_buffering: bool,
    pub download_speed_bps: f64,
    pub source_loading: bool,
    pub source_loading_key: Option<String>,
    pub source_load_error: Option<String>,
    pub speed: f64,
    pub volume: f64,
    pub muted: bool,
    pub tracks: Vec<MediaTrackDto>,
    pub playlist_position: i64,
    pub playlist_count: i64,
}

impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            playback_session_id: None,
            playback_key: None,
            playback_playlist_id: None,
            title: None,
            duration: 0.0,
            position: 0.0,
            buffered_position: 0.0,
            is_playing: false,
            is_buffering: false,
            download_speed_bps: 0.0,
            source_loading: false,
            source_loading_key: None,
            source_load_error: None,
            speed: 1.0,
            volume: 100.0,
            muted: false,
            tracks: Vec::new(),
            playlist_position: -1,
            playlist_count: 0,
        }
    }
}

impl PlaybackSnapshot {
    fn to_dto(&self, revision: u64) -> PlaybackSnapshotDto {
        PlaybackSnapshotDto {
            protocol_version: PROTOCOL_VERSION,
            revision,
            playback_session_id: self.playback_session_id.clone(),
            playback_key: self.playback_key.clone(),
            playback_playlist_id: self.playback_playlist_id.clone(),
            title: self.title.clone(),
            duration: self.duration,
            position: self.position,
            buffered_position: self.buffered_position,
            is_playing: self.is_playing,
            is_buffering: self.is_buffering,
            download_speed_bps: self.download_speed_bps,
            source_loading: self.source_loading,
            source_loading_key: self.source_loading_key.clone(),
            source_load_error: self.source_load_error.clone(),
            speed: self.speed,
            volume: self.volume,
            muted: self.muted,
            tracks: self.tracks.clone(),
            playlist_position: self.playlist_position,
            playlist_count: self.playlist_count,
        }
    }
}

pub(crate) struct PlaybackStatePublisher {
    state: Mutex<(u64, PlaybackSnapshot)>,
    changed: Condvar,
    sender: watch::Sender<PlaybackSnapshotDto>,
}

impl PlaybackStatePublisher {
    pub(crate) fn new() -> Self {
        let initial = PlaybackSnapshot::default().to_dto(0);
        let (sender, _) = watch::channel(initial);
        Self {
            state: Mutex::new((0, PlaybackSnapshot::default())),
            changed: Condvar::new(),
            sender,
        }
    }

    pub(crate) fn update(&self, update: impl FnOnce(&mut PlaybackSnapshot)) -> PlaybackSnapshotDto {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut state.1);
        state.0 = state.0.saturating_add(1);
        let snapshot = state.1.to_dto(state.0);
        self.sender.send_replace(snapshot.clone());
        self.changed.notify_all();
        snapshot
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<PlaybackSnapshotDto> {
        self.sender.subscribe()
    }

    pub(crate) fn current(&self) -> PlaybackSnapshotDto {
        self.sender.borrow().clone()
    }

    pub(crate) fn wait_for_revision_after(
        &self,
        revision: u64,
        timeout: Duration,
    ) -> Option<PlaybackSnapshotDto> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.0 <= revision {
            let remaining = deadline.checked_duration_since(Instant::now())?;
            let (next_state, result) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next_state;
            if result.timed_out() && state.0 <= revision {
                return None;
            }
        }
        Some(state.1.to_dto(state.0))
    }
}

#[cfg(test)]
mod tests {
    use super::PlaybackStatePublisher;

    #[test]
    fn publishes_playback_session_identity_in_snapshot() {
        let publisher = PlaybackStatePublisher::new();
        let published = publisher.update(|snapshot| {
            snapshot.playback_session_id = Some("session-b".to_string());
        });

        assert_eq!(published.protocol_version, 3);
        assert_eq!(published.playback_session_id.as_deref(), Some("session-b"));
        assert_eq!(
            publisher.current().playback_session_id.as_deref(),
            Some("session-b")
        );
    }
}
