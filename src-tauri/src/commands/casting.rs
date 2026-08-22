use crate::casting::{CastMediaSource, CastProtocolCommand};
use crate::protocol::{CastDeviceDto, CastErrorCodeDto, CastErrorDto, CastSnapshotDto};
use crate::AppState;
use std::net::Ipv4Addr;
use tauri::Emitter;

const CAST_SNAPSHOT_EVENT: &str = "cast-snapshot";
const CAST_DEVICES_EVENT: &str = "cast-devices";

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
    emit_snapshot(&app, &snapshot);
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
    emit_snapshot(&app, &state.casting_service.current_snapshot());
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
    let result = state.casting_service.disconnect().await;
    emit_snapshot(&app, &state.casting_service.current_snapshot());
    result
}

async fn execute_command(
    app: &tauri::AppHandle,
    state: &AppState,
    command: CastProtocolCommand,
) -> Result<CastSnapshotDto, CastErrorDto> {
    let result = state.casting_service.command(command).await;
    emit_snapshot(app, &state.casting_service.current_snapshot());
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

fn emit_snapshot(app: &tauri::AppHandle, snapshot: &CastSnapshotDto) {
    if let Err(error) = app.emit(CAST_SNAPSHOT_EVENT, snapshot) {
        log::warn!("failed to emit cast snapshot: {error}");
    }
}

fn emit_devices(app: &tauri::AppHandle, devices: &[CastDeviceDto]) {
    if let Err(error) = app.emit(CAST_DEVICES_EVENT, devices) {
        log::warn!("failed to emit cast devices: {error}");
    }
}
