use super::{macos, PlatformIntegration};
use crate::AppState;
use std::error::Error;

pub(super) struct DefaultPlatformIntegration;

impl PlatformIntegration for DefaultPlatformIntegration {
    fn new_state(&self) -> macos::PlatformState {
        Default::default()
    }

    fn setup(&self, _app: &mut tauri::App) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn cleanup_on_window_close(&self, _app_handle: &tauri::AppHandle) {}

    fn apply_now_playing_info(
        &self,
        _app_handle: &tauri::AppHandle,
        _state: &tauri::State<'_, AppState>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn clear_now_playing_cache(&self, _app_handle: &tauri::AppHandle) -> Result<(), String> {
        Ok(())
    }

    fn clear_now_playing_info(&self, _app_handle: &tauri::AppHandle) -> Result<(), String> {
        Ok(())
    }

    fn set_window_controls_visible(
        &self,
        _window: tauri::Window,
        _visible: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    fn apply_window_appearance(
        &self,
        window: tauri::Window,
        _compact_mode: bool,
        _corner_radius: Option<f64>,
        theme: Option<String>,
    ) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            let system_theme = window.theme().unwrap_or(tauri::Theme::Dark);
            super::windows::paint_native_window_background(
                &window,
                theme.as_deref(),
                system_theme,
            )
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (window, theme);
            Ok(())
        }
    }

    fn set_window_vibrancy_visible(
        &self,
        _window: tauri::Window,
        _visible: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    fn pick_media_paths_native(
        &self,
        _app_handle: tauri::AppHandle,
    ) -> Result<Vec<String>, String> {
        Err("Native media panel is only available on macOS".into())
    }
}
