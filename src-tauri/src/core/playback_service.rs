use crate::protocol::{
    CastPhaseDto, CastSnapshotDto, CommandEnvelopeDto, CommandResultDto, CoreErrorDto,
    PlaybackCommandDto,
};
use crate::{mpv_command_checked, AppState};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const COMMAND_RESULT_CACHE_CAPACITY: usize = 256;
const SNAPSHOT_UPDATE_TIMEOUT: Duration = Duration::from_secs(2);

/// The receiver currently authoritative for playback controls. This stays inside Core so clients
/// keep sending the same `PlaybackCommandDto` regardless of whether output is local or remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackOutputTarget {
    LocalMpv,
    Cast { session_id: String },
}

impl PlaybackOutputTarget {
    fn from_cast_snapshot(snapshot: &CastSnapshotDto) -> Self {
        let Some(session_id) = snapshot.session_id.as_ref() else {
            return Self::LocalMpv;
        };
        if matches!(
            snapshot.phase,
            CastPhaseDto::Playing
                | CastPhaseDto::Paused
                | CastPhaseDto::Buffering
                | CastPhaseDto::Stopped
        ) {
            return Self::Cast {
                session_id: session_id.clone(),
            };
        }
        Self::LocalMpv
    }
}

pub(crate) struct PlaybackService {
    sender: mpsc::Sender<QueuedCommand>,
    recent_results: Mutex<VecDeque<CachedCommandResult>>,
}

struct QueuedCommand {
    command: PlaybackCommandDto,
    response: mpsc::SyncSender<Result<(), CoreErrorDto>>,
}

struct CachedCommandResult {
    client_id: String,
    command_id: String,
    result: Result<CommandResultDto, CoreErrorDto>,
}

impl PlaybackService {
    pub(crate) fn new(mpv_player: Arc<Mutex<crate::mpv::MpvHandle>>) -> Self {
        let (sender, receiver) = mpsc::channel::<QueuedCommand>();
        std::thread::Builder::new()
            .name("soia-playback-command-queue".to_string())
            .spawn(move || {
                while let Ok(queued) = receiver.recv() {
                    let result = mpv_player
                        .lock()
                        .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })
                        .and_then(|mpv| execute_command(&mpv, queued.command));
                    let _ = queued.response.send(result);
                }
            })
            .expect("failed to start playback command queue");
        Self {
            sender,
            recent_results: Mutex::new(VecDeque::with_capacity(COMMAND_RESULT_CACHE_CAPACITY)),
        }
    }

    pub(crate) fn is_navigation_command(command: &PlaybackCommandDto) -> bool {
        matches!(
            command,
            PlaybackCommandDto::Previous
                | PlaybackCommandDto::Next
                | PlaybackCommandDto::PlaySource { .. }
        )
    }

    /// Resolves the active output without exposing protocol details to Tauri or WebSocket clients.
    /// A receiver becomes authoritative only after its load completed; connecting and loading keep
    /// local mpv active so a failed handoff cannot interrupt local playback.
    pub(crate) fn current_output_target(state: &AppState) -> PlaybackOutputTarget {
        PlaybackOutputTarget::from_cast_snapshot(&state.casting_service.current_snapshot())
    }

    /// The remote receiver is only made authoritative after it accepted the media. Keep this
    /// synchronous with the existing mpv command queue so a successful handoff cannot leave
    /// both outputs playing.
    pub(crate) fn pause_local_for_cast_handoff(&self) -> Result<(), CoreErrorDto> {
        self.execute_local_queued(PlaybackCommandDto::SetPaused { paused: true })
    }

    /// A finished cast leaves the original local source loaded but paused. Restore it to the
    /// last receiver-confirmed position without routing the commands back to the cast session.
    pub(crate) fn restore_local_after_cast(&self, position: f64) -> Result<(), CoreErrorDto> {
        self.execute_local_queued(PlaybackCommandDto::SeekAbsolute { position })?;
        self.execute_local_queued(PlaybackCommandDto::SetPaused { paused: true })
    }

    fn execute_local_queued(&self, command: PlaybackCommandDto) -> Result<(), CoreErrorDto> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(QueuedCommand {
                command,
                response: response_sender,
            })
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })?;
        response_receiver
            .recv()
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })?
    }

    pub(crate) async fn execute(
        &self,
        state: &AppState,
        envelope: CommandEnvelopeDto,
    ) -> Result<CommandResultDto, CoreErrorDto> {
        if envelope.command_id.trim().is_empty() || envelope.client_id.trim().is_empty() {
            return Err(CoreErrorDto::InvalidCommand { message: "commandId and clientId are required".into() });
        }
        if Self::is_navigation_command(&envelope.command) {
            return Err(CoreErrorDto::InvalidCommand {
                message: "navigation commands must be dispatched through the navigation service".into(),
            });
        }
        let _admission_lock = state.playback_command_lock.lock().await;
        if let Some(result) = self.cached_result(&envelope.client_id, &envelope.command_id) {
            return result;
        }

        let output_target = Self::current_output_target(state);
        if matches!(output_target, PlaybackOutputTarget::Cast { .. }) {
            let result = Err(CoreErrorDto::InvalidCommand {
                message: "playback commands for an active cast output are not available until a receiver adapter is installed".into(),
            });
            self.cache_result(&envelope.client_id, &envelope.command_id, result.clone());
            return result;
        }

        validate_playback_session(
            &envelope.command,
            envelope.playback_session_id.as_deref(),
            state
                .playback_state
                .current()
                .playback_session_id
                .as_deref(),
        )?;
        validate_track_selection(&envelope.command, &state.playback_state.current())?;

        let command = envelope.command.clone();
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(QueuedCommand { command: envelope.command, response: response_sender })
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })?;
        let result = response_receiver
            .recv()
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })?
            .and_then(|()| {
                let observed_snapshot = state.playback_state.current();
                if command_is_already_reflected(&command, &observed_snapshot) {
                    return Ok(CommandResultDto {
                        command_id: envelope.command_id.clone(),
                        applied_snapshot_revision: observed_snapshot.revision,
                    });
                }
                let snapshot = state
                    .playback_state
                    .wait_for_revision_after(observed_snapshot.revision, SNAPSHOT_UPDATE_TIMEOUT)
                    .ok_or_else(|| CoreErrorDto::ExecutionFailed {
                        message: "timed out waiting for mpv playback state update".into(),
                    })?;
                Ok(CommandResultDto {
                    command_id: envelope.command_id.clone(),
                    applied_snapshot_revision: snapshot.revision,
                })
            });
        self.cache_result(&envelope.client_id, &envelope.command_id, result.clone());
        result
    }

    fn cached_result(&self, client_id: &str, command_id: &str) -> Option<Result<CommandResultDto, CoreErrorDto>> {
        self.recent_results
            .lock()
            .ok()?
            .iter()
            .find(|entry| entry.client_id == client_id && entry.command_id == command_id)
            .map(|entry| entry.result.clone())
    }

    fn cache_result(
        &self,
        client_id: &str,
        command_id: &str,
        result: Result<CommandResultDto, CoreErrorDto>,
    ) {
        let Ok(mut cache) = self.recent_results.lock() else {
            return;
        };
        if cache.len() == COMMAND_RESULT_CACHE_CAPACITY {
            cache.pop_front();
        }
        cache.push_back(CachedCommandResult {
            client_id: client_id.to_string(),
            command_id: command_id.to_string(),
            result,
        });
    }
}

fn command_requires_playback_session(command: &PlaybackCommandDto) -> bool {
    matches!(
        command,
        PlaybackCommandDto::SeekAbsolute { .. }
            | PlaybackCommandDto::SeekRelative { .. }
            | PlaybackCommandDto::SelectAudioTrack { .. }
            | PlaybackCommandDto::SelectSubtitleTrack { .. }
            | PlaybackCommandDto::DisableSubtitles
    )
}

fn validate_track_selection(
    command: &PlaybackCommandDto,
    snapshot: &crate::protocol::PlaybackSnapshotDto,
) -> Result<(), CoreErrorDto> {
    let (track_id, expected_type) = match command {
        PlaybackCommandDto::SelectAudioTrack { track_id } => (*track_id, "audio"),
        PlaybackCommandDto::SelectSubtitleTrack { track_id } => (*track_id, "sub"),
        _ => return Ok(()),
    };
    if track_id <= 0 {
        return Err(CoreErrorDto::InvalidCommand {
            message: "trackId must be a positive integer".into(),
        });
    }
    if snapshot
        .tracks
        .iter()
        .any(|track| track.id == track_id && track.track_type == expected_type)
    {
        return Ok(());
    }
    Err(CoreErrorDto::InvalidCommand {
        message: format!(
            "{expected_type} track {track_id} is not available for the current media"
        ),
    })
}

fn validate_playback_session(
    command: &PlaybackCommandDto,
    requested_playback_session_id: Option<&str>,
    current_playback_session_id: Option<&str>,
) -> Result<(), CoreErrorDto> {
    if !command_requires_playback_session(command)
        || requested_playback_session_id == current_playback_session_id
    {
        return Ok(());
    }

    Err(CoreErrorDto::StalePlaybackSession {
        message: "playback session has changed".to_string(),
        requested_playback_session_id: requested_playback_session_id.map(str::to_string),
        current_playback_session_id: current_playback_session_id.map(str::to_string),
    })
}

fn command_is_already_reflected(command: &PlaybackCommandDto, snapshot: &crate::protocol::PlaybackSnapshotDto) -> bool {
    match command {
        PlaybackCommandDto::SetPaused { paused } => snapshot.is_playing == !paused,
        PlaybackCommandDto::SeekAbsolute { position } => {
            (snapshot.position - position).abs() < 0.25
        }
        PlaybackCommandDto::SetVolume { volume } => (snapshot.volume - volume.clamp(0.0, 130.0)).abs() < 0.25,
        PlaybackCommandDto::SetMuted { muted } => snapshot.muted == *muted,
        PlaybackCommandDto::SetSpeed { speed } => (snapshot.speed - speed).abs() < 0.001,
        PlaybackCommandDto::SelectAudioTrack { track_id } => snapshot
            .tracks
            .iter()
            .any(|track| track.track_type == "audio" && track.id == *track_id && track.selected),
        PlaybackCommandDto::SelectSubtitleTrack { track_id } => snapshot
            .tracks
            .iter()
            .any(|track| track.track_type == "sub" && track.id == *track_id && track.selected),
        PlaybackCommandDto::DisableSubtitles => !snapshot
            .tracks
            .iter()
            .any(|track| track.track_type == "sub" && track.selected),
        PlaybackCommandDto::SeekRelative { .. }
        | PlaybackCommandDto::Stop
        | PlaybackCommandDto::Previous
        | PlaybackCommandDto::Next
        | PlaybackCommandDto::PlaySource { .. } => false,
    }
}

fn execute_command(
    mpv: &crate::mpv::MpvHandle,
    command: PlaybackCommandDto,
) -> Result<(), CoreErrorDto> {
    let command_error = |error: String| CoreErrorDto::ExecutionFailed { message: error };
    match command {
        PlaybackCommandDto::SetPaused { paused } => {
            if !paused && mpv.eof_reached() {
                mpv_command_checked(mpv, &["seek", "0", "absolute", "exact"]).map_err(command_error)?;
            }
            mpv_command_checked(mpv, &["set", "pause", if paused { "yes" } else { "no" }]).map_err(command_error)
        }
        PlaybackCommandDto::SeekAbsolute { position } => {
            if !position.is_finite() || position < 0.0 {
                return Err(CoreErrorDto::InvalidCommand { message: "seek position must be a non-negative finite number".into() });
            }
            let position = position.to_string();
            mpv_command_checked(mpv, &["seek", &position, "absolute"]).map_err(command_error)?;
            if mpv.eof_reached() {
                mpv_command_checked(mpv, &["set", "pause", "no"]).map_err(command_error)?;
            }
            Ok(())
        }
        PlaybackCommandDto::SeekRelative { seconds } => {
            if !seconds.is_finite() || !(-600.0..=600.0).contains(&seconds) {
                return Err(CoreErrorDto::InvalidCommand { message: "relative seek must be finite and within 600 seconds".into() });
            }
            let seconds = seconds.to_string();
            mpv_command_checked(mpv, &["seek", &seconds, "relative"]).map_err(command_error)
        }
        PlaybackCommandDto::SetVolume { volume } => {
            if !volume.is_finite() {
                return Err(CoreErrorDto::InvalidCommand { message: "volume must be finite".into() });
            }
            let volume = volume.clamp(0.0, 130.0).to_string();
            mpv_command_checked(mpv, &["set", "volume", &volume]).map_err(command_error)
        }
        PlaybackCommandDto::SetMuted { muted } => {
            mpv_command_checked(mpv, &["set", "mute", if muted { "yes" } else { "no" }]).map_err(command_error)
        }
        PlaybackCommandDto::SetSpeed { speed } => {
            if !speed.is_finite() || !(0.01..=100.0).contains(&speed) {
                return Err(CoreErrorDto::InvalidCommand { message: "playback speed must be finite and between 0.01 and 100".into() });
            }
            let speed = speed.to_string();
            mpv_command_checked(mpv, &["set", "speed", &speed]).map_err(command_error)
        }
        PlaybackCommandDto::SelectAudioTrack { track_id } => {
            let track_id = track_id.to_string();
            mpv_command_checked(mpv, &["set", "aid", &track_id]).map_err(command_error)
        }
        PlaybackCommandDto::SelectSubtitleTrack { track_id } => {
            let track_id = track_id.to_string();
            mpv_command_checked(mpv, &["set", "sid", &track_id]).map_err(command_error)
        }
        PlaybackCommandDto::DisableSubtitles => {
            mpv_command_checked(mpv, &["set", "sid", "no"]).map_err(command_error)
        }
        PlaybackCommandDto::Stop => mpv_command_checked(mpv, &["stop"]).map_err(command_error),
        PlaybackCommandDto::Previous | PlaybackCommandDto::Next | PlaybackCommandDto::PlaySource { .. } => {
            Err(CoreErrorDto::InvalidCommand {
                message: "navigation commands must be handled by the navigation service".into(),
            })
        }
    }
}

#[cfg(test)]
mod playback_session_tests {
    use super::{
        validate_playback_session, validate_track_selection, PlaybackOutputTarget,
    };
    use crate::protocol::{
        CastPhaseDto, CastSnapshotDto, CoreErrorDto, MediaTrackDto, PlaybackCommandDto,
        PlaybackSnapshotDto,
    };

    #[test]
    fn accepts_seek_for_current_playback_session() {
        let result = validate_playback_session(
            &PlaybackCommandDto::SeekAbsolute { position: 42.0 },
            Some("session-b"),
            Some("session-b"),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_delayed_seek_for_replaced_playback_session() {
        let result = validate_playback_session(
            &PlaybackCommandDto::SeekRelative { seconds: 5.0 },
            Some("session-a"),
            Some("session-b"),
        );

        match result {
            Err(CoreErrorDto::StalePlaybackSession {
                message,
                requested_playback_session_id,
                current_playback_session_id,
            }) => {
                assert_eq!(message, "playback session has changed");
                assert_eq!(requested_playback_session_id.as_deref(), Some("session-a"));
                assert_eq!(current_playback_session_id.as_deref(), Some("session-b"));
            }
            _ => panic!("expected stale playback session error"),
        }
    }

    #[test]
    fn rejects_seek_without_session_after_media_is_loaded() {
        let result = validate_playback_session(
            &PlaybackCommandDto::SeekAbsolute { position: 42.0 },
            None,
            Some("session-b"),
        );

        assert!(matches!(
            result,
            Err(CoreErrorDto::StalePlaybackSession { .. })
        ));
    }

    #[test]
    fn does_not_require_session_for_last_write_wins_controls() {
        let result = validate_playback_session(
            &PlaybackCommandDto::SetVolume { volume: 80.0 },
            Some("session-a"),
            Some("session-b"),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_track_selection_for_replaced_playback_session() {
        let result = validate_playback_session(
            &PlaybackCommandDto::SelectAudioTrack { track_id: 1 },
            Some("session-a"),
            Some("session-b"),
        );

        assert!(matches!(
            result,
            Err(CoreErrorDto::StalePlaybackSession { .. })
        ));
    }

    #[test]
    fn rejects_track_selection_without_a_playback_session() {
        let result = validate_playback_session(
            &PlaybackCommandDto::SelectSubtitleTrack { track_id: 1 },
            None,
            Some("session-b"),
        );

        assert!(matches!(
            result,
            Err(CoreErrorDto::StalePlaybackSession { .. })
        ));
    }

    #[test]
    fn rejects_track_selection_when_only_the_wrong_track_type_has_the_id() {
        let snapshot = PlaybackSnapshotDto {
            tracks: vec![MediaTrackDto {
                id: 4,
                track_type: "sub".to_string(),
                ..MediaTrackDto::default()
            }],
            ..PlaybackSnapshotDto::default()
        };

        let result = validate_track_selection(
            &PlaybackCommandDto::SelectAudioTrack { track_id: 4 },
            &snapshot,
        );

        assert!(matches!(result, Err(CoreErrorDto::InvalidCommand { .. })));
    }

    #[test]
    fn accepts_duplicate_track_ids_when_the_requested_type_exists() {
        let snapshot = PlaybackSnapshotDto {
            tracks: vec![
                MediaTrackDto {
                    id: 1,
                    track_type: "video".to_string(),
                    ..MediaTrackDto::default()
                },
                MediaTrackDto {
                    id: 1,
                    track_type: "audio".to_string(),
                    ..MediaTrackDto::default()
                },
                MediaTrackDto {
                    id: 1,
                    track_type: "sub".to_string(),
                    ..MediaTrackDto::default()
                },
            ],
            ..PlaybackSnapshotDto::default()
        };

        assert!(validate_track_selection(
            &PlaybackCommandDto::SelectAudioTrack { track_id: 1 },
            &snapshot,
        )
        .is_ok());
        assert!(validate_track_selection(
            &PlaybackCommandDto::SelectSubtitleTrack { track_id: 1 },
            &snapshot,
        )
        .is_ok());
    }

    #[test]
    fn accepts_available_track_of_the_requested_type() {
        let snapshot = PlaybackSnapshotDto {
            tracks: vec![MediaTrackDto {
                id: 4,
                track_type: "sub".to_string(),
                ..MediaTrackDto::default()
            }],
            ..PlaybackSnapshotDto::default()
        };

        assert!(validate_track_selection(
            &PlaybackCommandDto::SelectSubtitleTrack { track_id: 4 },
            &snapshot,
        )
        .is_ok());
    }

    #[test]
    fn rejects_a_track_that_is_not_in_the_current_snapshot() {
        let result = validate_track_selection(
            &PlaybackCommandDto::SelectSubtitleTrack { track_id: 99 },
            &PlaybackSnapshotDto::default(),
        );

        assert!(matches!(result, Err(CoreErrorDto::InvalidCommand { .. })));
    }

    #[test]
    fn output_target_keeps_mpv_authoritative_until_receiver_load_is_confirmed() {
        for phase in [
            CastPhaseDto::Idle,
            CastPhaseDto::Discovering,
            CastPhaseDto::Connecting,
            CastPhaseDto::Loading,
            CastPhaseDto::Disconnected,
            CastPhaseDto::Error,
        ] {
            let snapshot = CastSnapshotDto {
                phase,
                session_id: Some("cast-a".to_string()),
                ..Default::default()
            };
            assert_eq!(
                PlaybackOutputTarget::from_cast_snapshot(&snapshot),
                PlaybackOutputTarget::LocalMpv,
            );
        }
    }

    #[test]
    fn output_target_uses_confirmed_cast_session_as_identity() {
        for phase in [
            CastPhaseDto::Playing,
            CastPhaseDto::Paused,
            CastPhaseDto::Buffering,
            CastPhaseDto::Stopped,
        ] {
            let snapshot = CastSnapshotDto {
                phase,
                session_id: Some("cast-b".to_string()),
                ..Default::default()
            };
            assert_eq!(
                PlaybackOutputTarget::from_cast_snapshot(&snapshot),
                PlaybackOutputTarget::Cast {
                    session_id: "cast-b".to_string(),
                },
            );
        }
    }

    #[test]
    fn output_target_requires_a_cast_session_identity() {
        let snapshot = CastSnapshotDto {
            phase: CastPhaseDto::Playing,
            ..Default::default()
        };

        assert_eq!(
            PlaybackOutputTarget::from_cast_snapshot(&snapshot),
            PlaybackOutputTarget::LocalMpv,
        );
    }
}
