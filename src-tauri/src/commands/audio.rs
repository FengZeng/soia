use crate::audio_output::{self, AudioDevice, AudioOutputStatus, AudioSettings};
use crate::{with_mpv, AppState};
use tauri::Manager;

#[tauri::command]
pub(crate) fn get_audio_settings(
    state: tauri::State<'_, AppState>,
) -> Result<AudioSettings, String> {
    Ok(state.audio_output.settings())
}

#[tauri::command]
pub(crate) fn apply_audio_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: AudioSettings,
) -> Result<AudioSettings, String> {
    let settings = settings.normalized();
    with_mpv(&state, |mpv| {
        audio_output::apply_to_mpv(mpv, &settings, true)
    })?;

    let status = state.audio_output.set_settings(settings.clone());
    crate::store::ui_state_store::save_audio_settings(&app, settings.clone())?;
    audio_output::emit_status(&app, status);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn get_audio_devices(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AudioDevice>, String> {
    Ok(state.audio_output.devices())
}

#[tauri::command]
pub(crate) fn get_audio_output_status(
    state: tauri::State<'_, AppState>,
) -> Result<AudioOutputStatus, String> {
    Ok(state.audio_output.status())
}

#[tauri::command]
pub(crate) fn retry_audio_output(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    with_mpv(&state, |mpv| {
        let result = mpv.command(&["ao-reload"]);
        if result >= 0 {
            Ok(())
        } else {
            Err(format!("mpv ao-reload failed with error {result}"))
        }
    })?;

    let status = app.state::<AppState>().audio_output.status();
    audio_output::emit_status(&app, status);
    Ok(())
}
