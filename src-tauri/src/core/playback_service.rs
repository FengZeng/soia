use crate::protocol::{CommandEnvelopeDto, CommandResultDto, CoreErrorDto, PlaybackCommandDto};
use crate::{mpv_command_checked, AppState};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const COMMAND_RESULT_CACHE_CAPACITY: usize = 256;
const SNAPSHOT_UPDATE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct PlaybackService {
    sender: mpsc::Sender<QueuedCommand>,
    admission_lock: Mutex<()>,
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
            admission_lock: Mutex::new(()),
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

    pub(crate) fn execute(
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
        let _admission_lock = self
            .admission_lock
            .lock()
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })?;
        if let Some(result) = self.cached_result(&envelope.client_id, &envelope.command_id) {
            return result;
        }

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

fn command_is_already_reflected(command: &PlaybackCommandDto, snapshot: &crate::protocol::PlaybackSnapshotDto) -> bool {
    match command {
        PlaybackCommandDto::SetPaused { paused } => snapshot.is_playing == !paused,
        PlaybackCommandDto::SeekAbsolute { position } => {
            (snapshot.position - position).abs() < 0.25
        }
        PlaybackCommandDto::SetVolume { volume } => (snapshot.volume - volume.clamp(0.0, 130.0)).abs() < 0.25,
        PlaybackCommandDto::SetMuted { muted } => snapshot.muted == *muted,
        PlaybackCommandDto::SetSpeed { speed } => (snapshot.speed - speed).abs() < 0.001,
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
        PlaybackCommandDto::Stop => mpv_command_checked(mpv, &["stop"]).map_err(command_error),
        PlaybackCommandDto::Previous | PlaybackCommandDto::Next | PlaybackCommandDto::PlaySource { .. } => {
            Err(CoreErrorDto::InvalidCommand {
                message: "navigation commands must be handled by the navigation service".into(),
            })
        }
    }
}
