use crate::AppState;
use std::error::Error;
#[cfg(target_os = "macos")]
use tauri::Manager;

pub(crate) mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod macos_ffi;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(not(target_os = "macos"))]
mod default;

pub(crate) trait PlatformIntegration: Sync {
    fn new_state(&self) -> macos::PlatformState;
    fn setup(&self, app: &mut tauri::App) -> Result<(), Box<dyn Error>>;
    fn cleanup_on_window_close(&self, app_handle: &tauri::AppHandle);
    fn apply_now_playing_info(
        &self,
        app_handle: &tauri::AppHandle,
        state: &tauri::State<'_, AppState>,
    ) -> Result<(), String>;
    fn clear_now_playing_cache(&self, app_handle: &tauri::AppHandle) -> Result<(), String>;
    fn clear_now_playing_info(&self, app_handle: &tauri::AppHandle) -> Result<(), String>;
    fn set_window_controls_visible(
        &self,
        window: tauri::Window,
        visible: bool,
    ) -> Result<(), String>;
    fn apply_window_appearance(
        &self,
        window: tauri::Window,
        compact_mode: bool,
        corner_radius: Option<f64>,
        theme: Option<String>,
    ) -> Result<(), String>;
    fn set_window_vibrancy_visible(
        &self,
        window: tauri::Window,
        visible: bool,
    ) -> Result<(), String>;
    fn pick_media_paths_native(&self, app_handle: tauri::AppHandle) -> Result<Vec<String>, String>;
}

#[cfg(target_os = "macos")]
struct MacPlatformIntegration;

#[cfg(target_os = "macos")]
impl PlatformIntegration for MacPlatformIntegration {
    fn new_state(&self) -> macos::PlatformState {
        Default::default()
    }

    fn setup(&self, app: &mut tauri::App) -> Result<(), Box<dyn Error>> {
        macos::setup(app)
    }

    fn cleanup_on_window_close(&self, app_handle: &tauri::AppHandle) {
        if let Some(state) = app_handle.try_state::<macos::PlatformState>() {
            macos::cleanup_on_window_close(app_handle, &state);
        }
    }

    fn apply_now_playing_info(
        &self,
        app_handle: &tauri::AppHandle,
        state: &tauri::State<'_, AppState>,
    ) -> Result<(), String> {
        macos::apply_now_playing_info(app_handle, state)
    }

    fn clear_now_playing_cache(&self, app_handle: &tauri::AppHandle) -> Result<(), String> {
        macos::clear_now_playing_cache(app_handle)
    }

    fn clear_now_playing_info(&self, app_handle: &tauri::AppHandle) -> Result<(), String> {
        macos::clear_now_playing_info(app_handle)
    }

    fn set_window_controls_visible(
        &self,
        window: tauri::Window,
        visible: bool,
    ) -> Result<(), String> {
        macos::set_window_controls_visible(window, visible)
    }

    fn apply_window_appearance(
        &self,
        window: tauri::Window,
        compact_mode: bool,
        corner_radius: Option<f64>,
        theme: Option<String>,
    ) -> Result<(), String> {
        macos::apply_window_appearance(window, compact_mode, corner_radius, theme)
    }

    fn set_window_vibrancy_visible(
        &self,
        window: tauri::Window,
        visible: bool,
    ) -> Result<(), String> {
        macos::set_window_vibrancy_visible(window, visible)
    }

    fn pick_media_paths_native(&self, app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
        macos::pick_media_paths_native(app_handle)
    }
}

#[cfg(target_os = "macos")]
static PLATFORM_INTEGRATION: MacPlatformIntegration = MacPlatformIntegration;

#[cfg(not(target_os = "macos"))]
use default::DefaultPlatformIntegration;

#[cfg(not(target_os = "macos"))]
static PLATFORM_INTEGRATION: DefaultPlatformIntegration = DefaultPlatformIntegration;

fn platform_integration() -> &'static dyn PlatformIntegration {
    &PLATFORM_INTEGRATION
}

pub(crate) fn new_platform_state() -> macos::PlatformState {
    platform_integration().new_state()
}

pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn Error>> {
    platform_integration().setup(app)
}

pub(crate) fn cleanup_on_window_close(app_handle: &tauri::AppHandle) {
    platform_integration().cleanup_on_window_close(app_handle);
}

pub(crate) fn apply_now_playing_info(
    app_handle: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
) -> Result<(), String> {
    platform_integration().apply_now_playing_info(app_handle, state)
}

pub(crate) fn apply_now_playing_status_async(app_handle: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    macos::apply_now_playing_status_async(app_handle);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app_handle;
    }
}

pub(crate) fn clear_now_playing_cache(app_handle: &tauri::AppHandle) -> Result<(), String> {
    platform_integration().clear_now_playing_cache(app_handle)
}

pub(crate) fn clear_now_playing_info(app_handle: &tauri::AppHandle) -> Result<(), String> {
    platform_integration().clear_now_playing_info(app_handle)
}

pub(crate) fn set_window_controls_visible(
    window: tauri::Window,
    visible: bool,
) -> Result<(), String> {
    platform_integration().set_window_controls_visible(window, visible)
}

pub(crate) fn apply_window_appearance(
    window: tauri::Window,
    compact_mode: bool,
    corner_radius: Option<f64>,
    theme: Option<String>,
) -> Result<(), String> {
    platform_integration().apply_window_appearance(window, compact_mode, corner_radius, theme)
}

pub(crate) fn set_window_vibrancy_visible(
    window: tauri::Window,
    visible: bool,
) -> Result<(), String> {
    platform_integration().set_window_vibrancy_visible(window, visible)
}

pub(crate) fn pick_media_paths_native(app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    platform_integration().pick_media_paths_native(app_handle)
}

pub(crate) fn is_native_pip_enabled(app_handle: &tauri::AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        return macos::is_native_pip_enabled(app_handle);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::is_native_pip_enabled(app_handle);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app_handle;
        false
    }
}

pub(crate) fn set_native_pip_enabled(
    app_handle: &tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return macos::set_native_pip_enabled(app_handle, enabled);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::set_native_pip_enabled(app_handle, enabled);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app_handle;
        let _ = enabled;
        Err("Native Picture in Picture is not available on this platform".into())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn update_native_pip_state(
    app_handle: &tauri::AppHandle,
    paused: bool,
    video_w: i64,
    video_h: i64,
) {
    #[cfg(target_os = "macos")]
    {
        macos::update_native_pip_state(app_handle, paused, video_w, video_h);
        return;
    }
    #[cfg(target_os = "windows")]
    {
        windows::update_native_pip_state(app_handle, paused, video_w, video_h);
        return;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (app_handle, paused, video_w, video_h);
    }
}

pub(crate) fn enforce_native_pip_aspect(
    window: &tauri::WebviewWindow,
    width: u32,
    height: u32,
) -> bool {
    #[cfg(target_os = "windows")]
    {
        return windows::enforce_native_pip_aspect(window, width, height);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, width, height);
        false
    }
}

pub(crate) async fn prepare_window_for_fullscreen(
    window: tauri::WebviewWindow,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        return windows::prepare_window_for_fullscreen(window).await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        Ok(false)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn sync_mpv_metal_layer_geometry(window: &tauri::WebviewWindow, utils: usize) {
    macos::sync_mpv_metal_layer_geometry(window, utils);
}
