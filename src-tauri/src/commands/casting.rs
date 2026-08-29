use crate::casting::{CastMediaSource, CastProtocolCommand};
use crate::protocol::{CastDeviceDto, CastErrorCodeDto, CastErrorDto, CastPhaseDto, CastSnapshotDto};
use crate::AppState;
use std::net::Ipv4Addr;
use tauri::{Emitter, Manager};
use tokio::time::{sleep, Duration};

const CAST_SNAPSHOT_EVENT: &str = "cast-snapshot";
const CAST_DEVICES_EVENT: &str = "cast-devices";
const CAST_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(750);
const CAST_STATUS_POLL_MAX_INTERVAL: Duration = Duration::from_secs(6);
const MAX_CONSECUTIVE_CAST_STATUS_FAILURES: u32 = 3;

#[tauri::command]
pub(crate) fn get_cast_snapshot(state: tauri::State<'_, AppState>) -> CastSnapshotDto {
    state.casting_service.current_snapshot()
}

#[tauri::command]
pub(crate) fn get_cast_devices(state: tauri::State<'_, AppState>) -> Vec<CastDeviceDto> {
    state.casting_service.devices()
}

#[tauri::command]
pub(crate) async fn discover_cast_devices(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CastDeviceDto>, CastErrorDto> {
    let result = state.casting_service.discover().await;
    let snapshot = state.casting_service.current_snapshot();
    emit_cast_snapshot(&app, &snapshot);
    if let Ok(devices) = &result {
        emit_devices(&app, devices);
    }
    result
}

#[tauri::command]
pub(crate) async fn connect_cast_device(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> Result<CastSnapshotDto, CastErrorDto> {
    let _playback_command_lock = state.playback_command_lock.lock().await;
    let playback = state.playback_state.current();
    let playback_key = playback.playback_key.clone().ok_or_else(|| media_error("no current playback source"))?;
    let source = crate::casting::source::resolve(&app, &playback_key)
        .await
        .map_err(|error| media_error(&error))?;
    let title = playback.title.clone();
    let duration = (playback.duration > 0.0).then_some(playback.duration);
    let position = playback.position.max(0.0);
    let app_for_media = app.clone();
    let result = state
        .casting_service
        .connect_and_load_with(&device_id, move |session_id, device| {
            let receiver_ip = device.address.parse::<Ipv4Addr>().map_err(|_| CastErrorDto {
                code: CastErrorCodeDto::DeviceUnsupported,
                message: "cast device does not have an IPv4 address".to_string(),
                device_id: Some(device.id.clone()),
            })?;
            create_descriptor(
                source,
                app_for_media.clone(),
                session_id,
                receiver_ip,
                title,
                duration,
                position,
            )
        })
        .await;
    let result = match result {
        Ok(snapshot) => match state.playback_service.pause_local_for_cast_handoff() {
            Ok(()) => Ok(snapshot),
            Err(error) => {
                log::error!(
                    "casting: receiver accepted media but local mpv could not be paused: {:?}",
                    error
                );
                let _ = state.casting_service.command(CastProtocolCommand::Stop).await;
                let _ = state.casting_service.disconnect().await;
                Err(CastErrorDto {
                    code: CastErrorCodeDto::LoadFailed,
                    message: "could not pause local playback after the receiver accepted media"
                        .to_string(),
                    device_id: Some(device_id),
                })
            }
        },
        Err(error) => Err(error),
    };
    emit_cast_snapshot(&app, &state.casting_service.current_snapshot());
    if result.is_ok() {
        if let Some(snapshot) = state.playback_service.activate_cast_output(&state) {
            crate::commands::playback::emit_playback_snapshot(&app, &snapshot);
        }
    }
    if let Ok(snapshot) = &result {
        if let Some(session_id) = snapshot.session_id.as_deref() {
            start_cast_status_poll(app.clone(), session_id.to_string());
        }
    }
    result
}

#[tauri::command]
pub(crate) async fn cast_play(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CastSnapshotDto, CastErrorDto> {
    execute_command(&app, &state, CastProtocolCommand::Play).await
}

#[tauri::command]
pub(crate) async fn cast_pause(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CastSnapshotDto, CastErrorDto> {
    execute_command(&app, &state, CastProtocolCommand::Pause).await
}

#[tauri::command]
pub(crate) async fn cast_seek(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    position: f64,
) -> Result<CastSnapshotDto, CastErrorDto> {
    execute_command(&app, &state, CastProtocolCommand::SeekAbsolute { position }).await
}

#[tauri::command]
pub(crate) async fn cast_stop(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CastSnapshotDto, CastErrorDto> {
    execute_command(&app, &state, CastProtocolCommand::Stop).await
}

#[tauri::command]
pub(crate) async fn set_cast_volume(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    volume: f64,
) -> Result<CastSnapshotDto, CastErrorDto> {
    execute_command(&app, &state, CastProtocolCommand::SetVolume { volume }).await
}

#[tauri::command]
pub(crate) async fn disconnect_casting(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CastSnapshotDto, CastErrorDto> {
    let _playback_command_lock = state.playback_command_lock.lock().await;
    let snapshot_before_stop = state.casting_service.current_snapshot();
    let should_restore_local = snapshot_before_stop.session_id.is_some()
        && matches!(
            snapshot_before_stop.phase,
            CastPhaseDto::Playing
                | CastPhaseDto::Paused
                | CastPhaseDto::Buffering
                | CastPhaseDto::Stopped
        );
    let last_position = if should_restore_local {
        state
            .casting_service
            .refresh_status()
            .await
            .map(|snapshot| snapshot.position)
            .unwrap_or(snapshot_before_stop.position)
    } else {
        0.0
    };
    if let Err(error) = state.casting_service.command(CastProtocolCommand::Stop).await {
        log::debug!("casting: remote stop before disconnect failed: {}", error.message);
    }
    let result = state.casting_service.disconnect().await;
    let result = match result {
        Ok(snapshot) if should_restore_local => match state
            .playback_service
            .restore_local_after_cast(last_position)
        {
            Ok(()) => {
                let playback_snapshot = state.playback_service.finish_cast_output(&state, last_position);
                crate::commands::playback::emit_playback_snapshot(&app, &playback_snapshot);
                Ok(snapshot)
            }
            Err(error) => Err(CastErrorDto {
                code: CastErrorCodeDto::CommandFailed,
                message: format!("could not restore local playback after casting: {error:?}"),
                device_id: snapshot_before_stop.device.map(|device| device.id),
            }),
        },
        other => other,
    };
    emit_cast_snapshot(&app, &state.casting_service.current_snapshot());
    result
}

async fn execute_command(
    app: &tauri::AppHandle,
    state: &AppState,
    command: CastProtocolCommand,
) -> Result<CastSnapshotDto, CastErrorDto> {
    let result = state.casting_service.command(command).await;
    emit_cast_snapshot(app, &state.casting_service.current_snapshot());
    if result.is_ok() {
        if let Some(snapshot) = state.playback_service.sync_cast_output(state) {
            crate::commands::playback::emit_playback_snapshot(app, &snapshot);
        }
    }
    result
}

fn create_descriptor(
    source: CastMediaSource,
    app: tauri::AppHandle,
    session_id: &str,
    receiver_ip: Ipv4Addr,
    title: Option<String>,
    duration: Option<f64>,
    position: f64,
) -> Result<crate::casting::CastMediaDescriptor, CastErrorDto> {
    source
        .create_descriptor(app, session_id, receiver_ip, title, duration, position)
        .map_err(|error| media_error(&error))
}

fn media_error(message: &str) -> CastErrorDto {
    CastErrorDto {
        code: CastErrorCodeDto::MediaUnavailable,
        message: message.to_string(),
        device_id: None,
    }
}

pub(crate) fn emit_cast_snapshot(app: &tauri::AppHandle, snapshot: &CastSnapshotDto) {
    if let Err(error) = app.emit(CAST_SNAPSHOT_EVENT, snapshot) {
        log::warn!("failed to emit cast snapshot: {error}");
    }
}

fn emit_devices(app: &tauri::AppHandle, devices: &[CastDeviceDto]) {
    if let Err(error) = app.emit(CAST_DEVICES_EVENT, devices) {
        log::warn!("failed to emit cast devices: {error}");
    }
}

fn start_cast_status_poll(app: tauri::AppHandle, session_id: String) {
    tauri::async_runtime::spawn(async move {
        let initial_snapshot = app.state::<AppState>().casting_service.current_snapshot();
        let mut has_been_active = matches!(
            initial_snapshot.phase,
            CastPhaseDto::Playing | CastPhaseDto::Buffering | CastPhaseDto::Paused
        );
        let mut last_confirmed_position = initial_snapshot.position.max(0.0);
        let mut consecutive_failures = 0;
        let mut poll_delay = CAST_STATUS_POLL_INTERVAL;
        loop {
            sleep(poll_delay).await;
            let state: tauri::State<'_, AppState> = app.state();
            let snapshot = state.casting_service.current_snapshot();
            if snapshot.session_id.as_deref() != Some(session_id.as_str())
                || !matches!(
                    snapshot.phase,
                    CastPhaseDto::Playing
                        | CastPhaseDto::Paused
                        | CastPhaseDto::Buffering
                        | CastPhaseDto::Stopped
                )
            {
                break;
            }
            let snapshot = match state.casting_service.refresh_status().await {
                Ok(snapshot) => {
                    consecutive_failures = 0;
                    poll_delay = CAST_STATUS_POLL_INTERVAL;
                    last_confirmed_position = snapshot.position.max(0.0);
                    snapshot
                }
                Err(error) => {
                    consecutive_failures += 1;
                    if consecutive_failures < MAX_CONSECUTIVE_CAST_STATUS_FAILURES {
                        let factor = 1u32 << consecutive_failures.min(3);
                        poll_delay = (CAST_STATUS_POLL_INTERVAL * factor)
                            .min(CAST_STATUS_POLL_MAX_INTERVAL);
                        log::debug!(
                            "casting: status poll {}/{} failed for {}: {}",
                            consecutive_failures,
                            MAX_CONSECUTIVE_CAST_STATUS_FAILURES,
                            session_id,
                            error.message
                        );
                        continue;
                    }
                    let _playback_command_lock = state.playback_command_lock.lock().await;
                    let current = state.casting_service.current_snapshot();
                    if current.session_id.as_deref() != Some(session_id.as_str()) {
                        break;
                    }
                    let device = current.device.as_ref();
                    let failure = CastErrorDto {
                        code: CastErrorCodeDto::DeviceDisconnected,
                        message: format!(
                            "{} stopped responding after {} status checks: {}",
                            device.map(|device| device.name.as_str()).unwrap_or("Cast receiver"),
                            MAX_CONSECUTIVE_CAST_STATUS_FAILURES,
                            error.message,
                        ),
                        device_id: device.map(|device| device.id.clone()),
                    };
                    let Some(disconnected) = state
                        .casting_service
                        .disconnect_after_status_failures(&session_id, failure)
                        .await
                    else {
                        break;
                    };
                    emit_cast_snapshot(&app, &disconnected);
                    if let Err(error) = state
                        .playback_service
                        .restore_local_after_cast(last_confirmed_position)
                    {
                        log::warn!(
                            "casting: could not restore local playback after receiver disconnect: {:?}",
                            error
                        );
                    }
                    let playback_snapshot = state
                        .playback_service
                        .finish_cast_output(&state, last_confirmed_position);
                    crate::commands::playback::emit_playback_snapshot(&app, &playback_snapshot);
                    break;
                }
            };
            emit_cast_snapshot(&app, &snapshot);
            if let Some(playback_snapshot) = state.playback_service.sync_cast_output(&state) {
                crate::commands::playback::emit_playback_snapshot(&app, &playback_snapshot);
            }
            if !has_been_active
                && matches!(
                    snapshot.phase,
                    CastPhaseDto::Playing | CastPhaseDto::Buffering | CastPhaseDto::Paused
                )
            {
                has_been_active = true;
            }
            if snapshot.session_id.as_deref() != Some(session_id.as_str())
                || matches!(snapshot.phase, CastPhaseDto::Error | CastPhaseDto::Disconnected)
            {
                break;
            }
            // Receiver transitioning to Stopped after real playback means EOF or an out-of-band
            // stop; per plan §6.4 release the remote session and pin local mpv to the last
            // confirmed position. Explicit user stop paths already run this flow themselves;
            // running it again here is idempotent.
            if has_been_active && matches!(snapshot.phase, CastPhaseDto::Stopped) {
                let _playback_command_lock = state.playback_command_lock.lock().await;
                let current = state.casting_service.current_snapshot();
                if current.session_id.as_deref() != Some(session_id.as_str())
                    || !matches!(current.phase, CastPhaseDto::Stopped)
                {
                    break;
                }
                let position = current.position.max(0.0);
                if let Err(error) = state.casting_service.disconnect().await {
                    log::debug!(
                        "casting: teardown after receiver stopped failed: {}",
                        error.message
                    );
                }
                if let Err(error) = state.playback_service.restore_local_after_cast(position) {
                    log::warn!(
                        "casting: could not restore local playback after receiver stopped: {:?}",
                        error
                    );
                }
                let playback_snapshot =
                    state.playback_service.finish_cast_output(&state, position);
                crate::commands::playback::emit_playback_snapshot(&app, &playback_snapshot);
                emit_cast_snapshot(&app, &state.casting_service.current_snapshot());
                break;
            }
        }
    });
}
