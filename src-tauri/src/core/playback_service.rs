use crate::protocol::{CommandEnvelopeDto, CommandResultDto, CoreErrorDto, PlaybackCommandDto};
use crate::{mpv_command_checked, AppState};
use std::sync::{mpsc, Arc, Mutex};

pub(crate) struct PlaybackService {
    sender: mpsc::Sender<QueuedCommand>,
}

struct QueuedCommand {
    command: PlaybackCommandDto,
    response: mpsc::SyncSender<Result<(), CoreErrorDto>>,
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
        Self { sender }
    }

    pub(crate) fn execute(
        &self,
        state: &AppState,
        envelope: CommandEnvelopeDto,
    ) -> Result<CommandResultDto, CoreErrorDto> {
        if envelope.command_id.trim().is_empty() || envelope.client_id.trim().is_empty() {
            return Err(CoreErrorDto::InvalidCommand { message: "commandId and clientId are required".into() });
        }
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(QueuedCommand { command: envelope.command, response: response_sender })
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })?;
        response_receiver
            .recv()
            .map_err(|error| CoreErrorDto::ExecutionFailed { message: error.to_string() })??;
        Ok(CommandResultDto {
            command_id: envelope.command_id,
            applied_snapshot_revision: state.playback_state.current().revision,
        })
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
        PlaybackCommandDto::Stop => mpv_command_checked(mpv, &["stop"]).map_err(command_error),
    }
}
