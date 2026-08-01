#[tauri::command]
pub(crate) fn set_window_controls_visible(
    window: tauri::Window,
    visible: bool,
) -> Result<(), String> {
    crate::platform::set_window_controls_visible(window, visible)
}

#[tauri::command]
pub(crate) fn apply_window_appearance(
    window: tauri::Window,
    compact_mode: bool,
    corner_radius: Option<f64>,
    theme: Option<String>,
) -> Result<(), String> {
    crate::platform::apply_window_appearance(window, compact_mode, corner_radius, theme)
}

#[tauri::command]
pub(crate) fn set_window_vibrancy_visible(
    window: tauri::Window,
    visible: bool,
) -> Result<(), String> {
    crate::platform::set_window_vibrancy_visible(window, visible)
}

#[tauri::command]
pub(crate) fn sync_mpv_render_target(
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    crate::app_bootstrap::sync_mpv_render_target_to_window(&window)
}

#[tauri::command]
pub(crate) async fn prepare_window_for_fullscreen(
    window: tauri::WebviewWindow,
) -> Result<bool, String> {
    crate::platform::prepare_window_for_fullscreen(window).await
}
