#[cfg(target_os = "macos")]
use crate::AppState;
#[cfg(target_os = "macos")]
use std::error::Error;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::ffi::CStr;
    use std::io;
    use std::sync::{mpsc, Mutex};
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::{Emitter, Manager};
    use tauri_plugin_opener::OpenerExt;

    use crate::mpv::SoiaUtils;
    use crate::platform::macos_ffi::{
        soia_sync_layer_geometry, soia_utils_apply_now_playing_info_values,
        soia_utils_apply_now_playing_status_values, soia_utils_clear_now_playing_cache,
        soia_utils_clear_now_playing_info, soia_utils_is_pip_enabled,
        soia_utils_register_media_remote, soia_utils_set_pip_enabled,
        soia_utils_set_pip_event_callback, soia_utils_update_pip_state_values,
    };

    use block2::ffi as block_ffi;
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_app_kit::NSAutoresizingMaskOptions;
    use objc2_app_kit::NSAppearance;
    use objc2_app_kit::NSAppearanceNameAqua;
    use objc2_app_kit::NSAppearanceNameDarkAqua;
    use objc2_app_kit::NSColor;
    use objc2_app_kit::NSView;
    use objc2_app_kit::NSVisualEffectBlendingMode;
    use objc2_app_kit::NSVisualEffectMaterial;
    use objc2_app_kit::NSVisualEffectState;
    use objc2_app_kit::NSVisualEffectView;
    use objc2_app_kit::NSWindowButton;
    use objc2_app_kit::NSWindowOrderingMode;
    use objc2_app_kit::NSWindowStyleMask;
    use objc2_app_kit::NSWindowTitleVisibility;
    use objc2_app_kit::{
        NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
    };
    use objc2_foundation::{MainThreadMarker, NSNotification, NSNotificationCenter, NSString};

    pub(crate) struct FullscreenObserversRaw {
        pub(crate) enter_observer: usize,
        pub(crate) exit_observer: usize,
        pub(crate) enter_block: usize,
        pub(crate) exit_block: usize,
    }

    #[derive(Default)]
    pub(crate) struct PlatformState {
        pub(crate) fullscreen_observers: Mutex<Option<FullscreenObserversRaw>>,
        // Strongly-retained CAMetalLayer pointer created for mpv in the main window.
        // Stored as a raw pointer for cross-thread access; ownership is managed by retain/release
        // on the Objective-C side.
        pub(crate) pip_event_ctx: Mutex<Option<usize>>,
    }

    const SEEK_STEP_SETTING_LABEL: &str = "Skip Step";
    const APP_MENU_GITHUB_ITEM_ID: &str = "app.github";
    const APP_MENU_CHECK_UPDATE_ITEM_ID: &str = "app.check-update";
    const OPEN_SETTINGS_PANEL_EVENT: &str = "soia-open-settings-panel";
    const PROJECT_GITHUB_URL: &str = "https://github.com/FengZeng/soia";
    const WINDOW_VIBRANCY_VIEW_IDENTIFIER: &str = "soia-window-vibrancy-background";

    struct PipEventCtx {
        app_handle: tauri::AppHandle,
    }

    struct PendingNowPlayingStatusUpdate {
        scheduled: bool,
        dirty: bool,
    }

    static PENDING_NOW_PLAYING_STATUS_UPDATE: Mutex<PendingNowPlayingStatusUpdate> =
        Mutex::new(PendingNowPlayingStatusUpdate {
            scheduled: false,
            dirty: false,
        });

    extern "C" fn pip_event_on_change(ctx: *mut c_void, enabled: i32) {
        if ctx.is_null() {
            return;
        }
        let ctx = unsafe { &*(ctx as *mut PipEventCtx) };
        let _ = ctx.app_handle.emit("native-pip-changed", enabled != 0);
    }

    fn pip_seek_step_seconds(app_handle: &tauri::AppHandle) -> f64 {
        let fallback = 5.0;
        let Ok(value) =
            crate::store::ui_state_store::load_setting_value(app_handle, SEEK_STEP_SETTING_LABEL)
        else {
            return fallback;
        };
        let Some(raw) = value else {
            return fallback;
        };
        let parsed = raw.trim().parse::<f64>().ok();
        match parsed {
            Some(v) if v > 0.0 => v,
            _ => fallback,
        }
    }

    struct NowPlayingPayload {
        title: String,
        duration: f64,
        position: f64,
        is_playing: bool,
        artwork_path: Option<String>,
    }

    impl NowPlayingPayload {
        fn from_state(state: &tauri::State<'_, AppState>) -> Result<Self, String> {
            let snapshot = state.now_playing.lock().map_err(|e| e.to_string())?.clone();
            Ok(Self {
                title: snapshot
                    .title
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "Soia".to_string()),
                duration: snapshot.duration.unwrap_or(0.0),
                position: snapshot.position.max(0.0),
                is_playing: snapshot.is_playing,
                artwork_path: snapshot.artwork_path.filter(|value| !value.is_empty()),
            })
        }
    }

    fn run_on_main_thread_async(
        app_handle: &tauri::AppHandle,
        task: impl FnOnce() + Send + 'static,
    ) -> bool {
        app_handle.clone().run_on_main_thread(task).is_ok()
    }

    fn run_on_main_thread_sync<T: Send + 'static>(
        app_handle: &tauri::AppHandle,
        task: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = mpsc::channel::<Result<T, String>>();
        app_handle
            .clone()
            .run_on_main_thread(move || {
                let _ = tx.send(task());
            })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    unsafe fn find_window_vibrancy_view(content_view: &NSView) -> Option<Retained<NSView>> {
        let target_identifier = NSString::from_str(WINDOW_VIBRANCY_VIEW_IDENTIFIER);
        let subviews = content_view.subviews();
        let count = subviews.count();

        for index in 0..count {
            let subview = subviews.objectAtIndex(index);
            let identifier: *mut NSString = msg_send![&*subview, identifier];
            if identifier.is_null() {
                continue;
            }

            let matches_identifier: bool =
                msg_send![identifier, isEqualToString: &*target_identifier];
            if matches_identifier {
                return Some(subview);
            }
        }

        None
    }

    unsafe fn configure_window_vibrancy_view(view: &AnyObject, frame: objc2_foundation::NSRect) {
        let _: () = msg_send![view, setFrame: frame];
        let _: () = msg_send![
            view,
            setAutoresizingMask: NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable
        ];
        let _: () = msg_send![
            view,
            setMaterial: NSVisualEffectMaterial::UnderWindowBackground
        ];
        let _: () = msg_send![view, setBlendingMode: NSVisualEffectBlendingMode::BehindWindow];
        let _: () = msg_send![view, setState: NSVisualEffectState::Active];
        let _: () = msg_send![view, setEmphasized: false];
    }

    unsafe fn install_window_vibrancy_background(
        ns_window: &objc2_app_kit::NSWindow,
    ) -> Result<(), String> {
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&NSColor::clearColor()));

        let content_view = ns_window
            .contentView()
            .ok_or("Window has no content view for vibrancy background")?;
        let content_bounds = content_view.bounds();
        content_view.setAutoresizesSubviews(true);

        if let Some(existing_view) = find_window_vibrancy_view(&content_view) {
            let existing_object = &*existing_view as *const NSView as *const AnyObject;
            configure_window_vibrancy_view(
                existing_object
                    .as_ref()
                    .ok_or("Invalid existing vibrancy view pointer")?,
                content_bounds,
            );
            return Ok(());
        }

        let mtm = MainThreadMarker::new().ok_or("Vibrancy install is not on main thread")?;
        let vibrancy_view = NSVisualEffectView::new(mtm);
        configure_window_vibrancy_view(&vibrancy_view, content_bounds);

        let identifier = NSString::from_str(WINDOW_VIBRANCY_VIEW_IDENTIFIER);
        let _: () = msg_send![&*vibrancy_view, setIdentifier: &*identifier];
        content_view.addSubview_positioned_relativeTo(
            &vibrancy_view,
            NSWindowOrderingMode::Below,
            None,
        );

        Ok(())
    }

    unsafe fn apply_native_window_theme(
        ns_window: &objc2_app_kit::NSWindow,
        theme: Option<&str>,
    ) {
        let appearance_name = match theme {
            Some("dark") | Some("graphite") => Some(NSAppearanceNameDarkAqua),
            Some("light") => Some(NSAppearanceNameAqua),
            _ => None,
        };
        let appearance = appearance_name.and_then(NSAppearance::appearanceNamed);
        let appearance_ref = appearance.as_deref();
        let _: () = msg_send![ns_window, setAppearance: appearance_ref];
    }

    fn soia_utils_ptr(app_handle: &tauri::AppHandle) -> Option<*mut SoiaUtils> {
        let app_state: tauri::State<'_, AppState> = app_handle.state::<AppState>();
        let Ok(mpv_guard) = app_state.mpv_player.lock() else {
            return None;
        };
        let soia_utils = mpv_guard.soia_utils_ptr();
        if soia_utils.is_null() {
            None
        } else {
            Some(soia_utils)
        }
    }

    fn with_soia_utils_on_main_thread(
        app_handle: &tauri::AppHandle,
        task: impl FnOnce(*mut SoiaUtils) + Send + 'static,
    ) {
        let main_thread_handle = app_handle.clone();
        let state_handle = main_thread_handle.clone();
        let _ = run_on_main_thread_async(&main_thread_handle, move || {
            let Some(soia_utils) = soia_utils_ptr(&state_handle) else {
                return;
            };
            task(soia_utils);
        });
    }

    fn with_soia_utils_on_main_thread_result<T: Send + 'static>(
        app_handle: &tauri::AppHandle,
        task: impl FnOnce(*mut SoiaUtils) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let main_thread_handle = app_handle.clone();
        let state_handle = main_thread_handle.clone();
        run_on_main_thread_sync(&main_thread_handle, move || {
            let Some(soia_utils) = soia_utils_ptr(&state_handle) else {
                return Err("SoiaUtils not initialized".to_string());
            };
            task(soia_utils)
        })
    }

    fn with_soia_utils_on_main_thread_noop_if_missing(
        app_handle: &tauri::AppHandle,
        task: impl FnOnce(*mut SoiaUtils) + Send + 'static,
    ) -> Result<(), String> {
        let main_thread_handle = app_handle.clone();
        let state_handle = main_thread_handle.clone();
        run_on_main_thread_sync(&main_thread_handle, move || {
            let Some(soia_utils) = soia_utils_ptr(&state_handle) else {
                return Ok(());
            };
            task(soia_utils);
            Ok(())
        })
    }

    fn register_pip_event_callback(app_handle: &tauri::AppHandle) {
        let app_handle_cl = app_handle.clone();
        with_soia_utils_on_main_thread(app_handle, move |soia_utils| unsafe {
            let Some(state) = app_handle_cl.try_state::<PlatformState>() else {
                return;
            };
            let Ok(mut slot) = state.pip_event_ctx.lock() else {
                return;
            };

            let ctx = Box::new(PipEventCtx {
                app_handle: app_handle_cl.clone(),
            });
            let ctx_ptr = Box::into_raw(ctx) as *mut c_void;
            if let Some(old) = slot.replace(ctx_ptr as usize) {
                let _ = Box::from_raw(old as *mut PipEventCtx);
            }
            soia_utils_set_pip_event_callback(soia_utils, ctx_ptr, Some(pip_event_on_change));
        });
    }

    fn unregister_pip_event_callback(app_handle: &tauri::AppHandle) {
        let _ =
            with_soia_utils_on_main_thread_noop_if_missing(app_handle, move |soia_utils| unsafe {
                soia_utils_set_pip_event_callback(soia_utils, std::ptr::null_mut(), None);
            });
    }

    fn register_media_remote(app_handle: &tauri::AppHandle) {
        with_soia_utils_on_main_thread(app_handle, move |soia_utils| unsafe {
            let _ = soia_utils_register_media_remote(soia_utils);
        });
    }

    pub(crate) fn update_native_pip_state(
        app_handle: &tauri::AppHandle,
        paused: bool,
        video_w: i64,
        video_h: i64,
    ) {
        let seek_step = pip_seek_step_seconds(app_handle);
        with_soia_utils_on_main_thread(app_handle, move |soia_utils| unsafe {
            let _ = soia_utils_update_pip_state_values(
                soia_utils,
                if paused { 1 } else { 0 },
                video_w as f64,
                video_h as f64,
                seek_step,
            );
        });
    }

    fn release_block_raw(raw: usize) {
        if raw != 0 {
            unsafe {
                block_ffi::_Block_release(raw as *mut c_void);
            }
        }
    }

    pub(crate) fn sync_mpv_metal_layer_geometry(window: &tauri::WebviewWindow, utils: usize) {
        let app_handle = window.app_handle();
        let window = window.clone();
        let _ = app_handle.run_on_main_thread(move || unsafe {
            let Ok(ns_view_ptr) = window.ns_view() else {
                return;
            };
            soia_sync_layer_geometry(ns_view_ptr as *mut c_void, utils);
        });
    }

    pub(crate) fn clear_now_playing_cache(app_handle: &tauri::AppHandle) -> Result<(), String> {
        with_soia_utils_on_main_thread_noop_if_missing(app_handle, move |soia_utils| unsafe {
            let _ = soia_utils_clear_now_playing_cache(soia_utils);
        })
    }
    fn cleanup_fullscreen_observers(state: &PlatformState) {
        if let Ok(mut obs) = state.fullscreen_observers.lock() {
            if let Some(obs) = obs.take() {
                unsafe {
                    let center = NSNotificationCenter::defaultCenter();
                    if obs.enter_observer != 0 {
                        let enter_ptr = obs.enter_observer as *mut AnyObject;
                        center.removeObserver(&*enter_ptr);
                        let _ = Retained::from_raw(enter_ptr);
                    }
                    if obs.exit_observer != 0 {
                        let exit_ptr = obs.exit_observer as *mut AnyObject;
                        center.removeObserver(&*exit_ptr);
                        let _ = Retained::from_raw(exit_ptr);
                    }
                }
                release_block_raw(obs.enter_block);
                release_block_raw(obs.exit_block);
            }
        }
    }

    fn cleanup_pip_event_ctx(state: &PlatformState) {
        if let Ok(mut ctx) = state.pip_event_ctx.lock() {
            if let Some(ptr) = ctx.take() {
                unsafe {
                    let _ = Box::from_raw(ptr as *mut PipEventCtx);
                }
            }
        }
    }

    fn cleanup_macos_resources(state: &PlatformState) {
        cleanup_fullscreen_observers(state);
        cleanup_pip_event_ctx(state);
    }

    fn normalize_mpv_version(raw: Option<&str>) -> String {
        let Some(value) = raw else {
            return "Unavailable".to_string();
        };
        let trimmed = value.trim();
        let normalized = trimmed.strip_prefix("mpv ").unwrap_or(trimmed);
        if normalized.is_empty() {
            "Unavailable".to_string()
        } else {
            normalized.to_string()
        }
    }

    fn normalize_ffmpeg_version(raw: Option<&str>) -> String {
        let Some(value) = raw else {
            return "Unavailable".to_string();
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            "Unavailable".to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn build_about_credits(mpv_version: Option<&str>, ffmpeg_version: Option<&str>) -> String {
        let mpv = normalize_mpv_version(mpv_version);
        let ffmpeg = normalize_ffmpeg_version(ffmpeg_version);
        format!("mpv: {mpv}\nFFmpeg: {ffmpeg}")
    }

    fn configure_about_menu(
        app: &tauri::App,
        mpv_version: Option<String>,
        ffmpeg_version: Option<String>,
    ) -> Result<(), tauri::Error> {
        let app_handle = app.handle().clone();
        let pkg_info = app_handle.package_info();
        let config = app_handle.config();
        let about_metadata = AboutMetadata {
            name: Some(pkg_info.name.clone()),
            copyright: config.bundle.copyright.clone(),
            credits: Some(build_about_credits(
                mpv_version.as_deref(),
                ffmpeg_version.as_deref(),
            )),
            icon: Some(tauri::include_image!("./icons/128x128@2x.png")),
            ..Default::default()
        };

        let menu = Menu::with_items(
            &app_handle,
            &[
                &Submenu::with_items(
                    &app_handle,
                    pkg_info.name.clone(),
                    true,
                    &[
                        &PredefinedMenuItem::about(&app_handle, None, Some(about_metadata))?,
                        &MenuItem::with_id(
                            &app_handle,
                            APP_MENU_GITHUB_ITEM_ID,
                            "GitHub",
                            true,
                            None::<&str>,
                        )?,
                        &MenuItem::with_id(
                            &app_handle,
                            APP_MENU_CHECK_UPDATE_ITEM_ID,
                            "Check Update",
                            true,
                            None::<&str>,
                        )?,
                        &PredefinedMenuItem::separator(&app_handle)?,
                        &PredefinedMenuItem::services(&app_handle, None)?,
                        &PredefinedMenuItem::separator(&app_handle)?,
                        &PredefinedMenuItem::hide(&app_handle, None)?,
                        &PredefinedMenuItem::hide_others(&app_handle, None)?,
                        &PredefinedMenuItem::separator(&app_handle)?,
                        &PredefinedMenuItem::quit(&app_handle, None)?,
                    ],
                )?,
                &Submenu::with_items(
                    &app_handle,
                    "File",
                    true,
                    &[&PredefinedMenuItem::close_window(&app_handle, None)?],
                )?,
                &Submenu::with_items(
                    &app_handle,
                    "Edit",
                    true,
                    &[
                        &PredefinedMenuItem::undo(&app_handle, None)?,
                        &PredefinedMenuItem::redo(&app_handle, None)?,
                        &PredefinedMenuItem::separator(&app_handle)?,
                        &PredefinedMenuItem::cut(&app_handle, None)?,
                        &PredefinedMenuItem::copy(&app_handle, None)?,
                        &PredefinedMenuItem::paste(&app_handle, None)?,
                        &PredefinedMenuItem::select_all(&app_handle, None)?,
                    ],
                )?,
                &Submenu::with_items(
                    &app_handle,
                    "View",
                    true,
                    &[&PredefinedMenuItem::fullscreen(&app_handle, None)?],
                )?,
                &Submenu::with_items(
                    &app_handle,
                    "Window",
                    true,
                    &[
                        &PredefinedMenuItem::minimize(&app_handle, None)?,
                        &PredefinedMenuItem::maximize(&app_handle, None)?,
                        &PredefinedMenuItem::separator(&app_handle)?,
                        &PredefinedMenuItem::close_window(&app_handle, None)?,
                    ],
                )?,
            ],
        )?;

        app_handle.on_menu_event(|app, event| {
            if event.id() == APP_MENU_GITHUB_ITEM_ID {
                let _ = app.opener().open_url(PROJECT_GITHUB_URL, None::<&str>);
                return;
            }
            if event.id() == APP_MENU_CHECK_UPDATE_ITEM_ID {
                let _ = app.emit(OPEN_SETTINGS_PANEL_EVENT, ());
                crate::check_update::check_update_now(app.clone());
            }
        });

        app_handle.set_menu(menu)?;
        Ok(())
    }

    pub(crate) fn apply_now_playing_info(
        app_handle: &tauri::AppHandle,
        state: &tauri::State<'_, AppState>,
    ) -> Result<(), String> {
        let NowPlayingPayload {
            title,
            duration,
            position,
            is_playing,
            artwork_path,
        } = NowPlayingPayload::from_state(state)?;
        with_soia_utils_on_main_thread_noop_if_missing(app_handle, move |soia_utils| unsafe {
            let c_title = std::ffi::CString::new(title).ok();
            let c_art = artwork_path.and_then(|path| std::ffi::CString::new(path).ok());
            let title_ptr = c_title
                .as_ref()
                .map(|v| v.as_ptr())
                .unwrap_or(std::ptr::null());
            let art_ptr = c_art
                .as_ref()
                .map(|v| v.as_ptr())
                .unwrap_or(std::ptr::null());
            let _ = soia_utils_apply_now_playing_info_values(
                soia_utils,
                title_ptr,
                duration,
                position,
                if is_playing { 1 } else { 0 },
                art_ptr,
            );
        })
    }

    pub(crate) fn apply_now_playing_status_async(app_handle: &tauri::AppHandle) {
        let should_schedule = {
            let mut pending = PENDING_NOW_PLAYING_STATUS_UPDATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.dirty = true;
            if pending.scheduled {
                false
            } else {
                pending.scheduled = true;
                true
            }
        };
        if !should_schedule {
            return;
        }

        let state_handle = app_handle.clone();
        let scheduled = run_on_main_thread_async(app_handle, move || {
            loop {
                let should_apply = {
                    let mut pending = PENDING_NOW_PLAYING_STATUS_UPDATE
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if pending.dirty {
                        pending.dirty = false;
                        true
                    } else {
                        pending.scheduled = false;
                        false
                    }
                };
                if !should_apply {
                    return;
                }

                let Some(state) = state_handle.try_state::<AppState>() else {
                    continue;
                };
                let Ok(NowPlayingPayload {
                    title,
                    duration,
                    position,
                    is_playing,
                    ..
                }) = NowPlayingPayload::from_state(&state) else {
                    continue;
                };
                let Some(soia_utils) = soia_utils_ptr(&state_handle) else {
                    continue;
                };
                unsafe {
                    let c_title = std::ffi::CString::new(title).ok();
                    let title_ptr = c_title
                        .as_ref()
                        .map(|value| value.as_ptr())
                        .unwrap_or(std::ptr::null());
                    let _ = soia_utils_apply_now_playing_status_values(
                        soia_utils,
                        title_ptr,
                        duration,
                        position,
                        if is_playing { 1 } else { 0 },
                    );
                }
            }
        });
        if !scheduled {
            let mut pending = PENDING_NOW_PLAYING_STATUS_UPDATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.scheduled = false;
            pending.dirty = false;
        }
    }

    pub(crate) fn clear_now_playing_info(app_handle: &tauri::AppHandle) -> Result<(), String> {
        with_soia_utils_on_main_thread_noop_if_missing(app_handle, move |soia_utils| unsafe {
            let _ = soia_utils_clear_now_playing_info(soia_utils);
        })
    }

    pub(crate) fn set_window_controls_visible(
        window: tauri::Window,
        visible: bool,
    ) -> Result<(), String> {
        let app_handle = window.app_handle();
        let window_for_thread = window.clone();
        run_on_main_thread_sync(&app_handle, move || unsafe {
            let ns_window = window_for_thread
                .ns_window()
                .map_err(|e| format!("ns_window error: {e}"))?;
            let ns_window = (ns_window as *mut objc2_app_kit::NSWindow)
                .as_ref()
                .ok_or("Invalid NSWindow pointer")?;
            let buttons = [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ];
            for button in buttons {
                if let Some(btn) = ns_window.standardWindowButton(button) {
                    let _: () = msg_send![&*btn, setHidden: !visible];
                }
            }
            Ok(())
        })
    }

    pub(crate) fn is_native_pip_enabled(app_handle: &tauri::AppHandle) -> bool {
        let Some(soia_utils) = soia_utils_ptr(app_handle) else {
            return false;
        };
        unsafe { soia_utils_is_pip_enabled(soia_utils) != 0 }
    }

    pub(crate) fn set_native_pip_enabled(
        app_handle: &tauri::AppHandle,
        enabled: bool,
    ) -> Result<(), String> {
        let app_handle_cl = app_handle.clone();
        with_soia_utils_on_main_thread_result(app_handle, move |soia_utils| {
            let result =
                unsafe { soia_utils_set_pip_enabled(soia_utils, if enabled { 1 } else { 0 }) };
            if result != 0 {
                return Err("Failed to toggle native PiP".to_string());
            }

            let _ = app_handle_cl.emit("native-pip-changed", enabled);
            Ok(())
        })
    }

    pub(crate) fn apply_window_appearance(
        window: tauri::Window,
        compact_mode: bool,
        corner_radius: Option<f64>,
        theme: Option<String>,
    ) -> Result<(), String> {
        let app_handle = window.app_handle();
        let window_for_thread = window.clone();
        run_on_main_thread_sync(&app_handle, move || unsafe {
            let ns_window = window_for_thread
                .ns_window()
                .map_err(|e| format!("ns_window error: {e}"))?;
            let ns_window = (ns_window as *mut objc2_app_kit::NSWindow)
                .as_ref()
                .ok_or("Invalid NSWindow pointer")?;

            apply_native_window_theme(ns_window, theme.as_deref());

            if compact_mode {
                ns_window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
                ns_window.setTitlebarAppearsTransparent(true);
                let mask = ns_window.styleMask() | NSWindowStyleMask::FullSizeContentView;
                ns_window.setStyleMask(mask);
            } else {
                ns_window.setTitleVisibility(NSWindowTitleVisibility::Visible);
                ns_window.setTitlebarAppearsTransparent(false);
                let mask = ns_window.styleMask() & !NSWindowStyleMask::FullSizeContentView;
                ns_window.setStyleMask(mask);
            }

            let ns_view = window_for_thread
                .ns_view()
                .map_err(|e| format!("ns_view error: {e}"))?;
            let ns_view = (ns_view as *mut NSView)
                .as_ref()
                .ok_or("Invalid NSView pointer")?;

            let _: () = msg_send![ns_view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![ns_view, layer];
            if layer.is_null() {
                return Ok(());
            }

            let clamped_radius = corner_radius.unwrap_or(0.0).max(0.0);
            let rounded = clamped_radius > 0.0;
            let _: () = msg_send![layer, setCornerRadius: clamped_radius];
            let _: () = msg_send![layer, setMasksToBounds: rounded];
            let _: () = msg_send![layer, setAllowsEdgeAntialiasing: rounded];

            Ok(())
        })
    }

    pub(crate) fn set_window_vibrancy_visible(
        window: tauri::Window,
        visible: bool,
    ) -> Result<(), String> {
        let app_handle = window.app_handle();
        let window_for_thread = window.clone();
        run_on_main_thread_sync(&app_handle, move || unsafe {
            let ns_window = window_for_thread
                .ns_window()
                .map_err(|e| format!("ns_window error: {e}"))?;
            let ns_window = (ns_window as *mut objc2_app_kit::NSWindow)
                .as_ref()
                .ok_or("Invalid NSWindow pointer")?;

            let Some(content_view) = ns_window.contentView() else {
                return Ok(());
            };

            if visible {
                install_window_vibrancy_background(ns_window)?;
            } else if let Some(vibrancy_view) = find_window_vibrancy_view(&content_view) {
                let _: () = msg_send![&*vibrancy_view, removeFromSuperview];
            }

            Ok(())
        })
    }

    pub(crate) fn pick_media_paths_native(
        app_handle: tauri::AppHandle,
    ) -> Result<Vec<String>, String> {
        run_on_main_thread_sync(&app_handle, move || unsafe {
            let panel_class = AnyClass::get(std::ffi::CStr::from_bytes_with_nul_unchecked(
                b"NSOpenPanel\0",
            ))
            .ok_or("NSOpenPanel not available")?;
            let panel: *mut AnyObject = msg_send![panel_class, openPanel];

            let _: () = msg_send![panel, setCanChooseFiles: true];
            let _: () = msg_send![panel, setCanChooseDirectories: true];
            let _: () = msg_send![panel, setAllowsMultipleSelection: true];
            let _: () = msg_send![panel, setResolvesAliases: true];

            let response: i64 = msg_send![panel, runModal];
            if response != 1 {
                return Ok(Vec::new());
            }

            let urls: *mut AnyObject = msg_send![panel, URLs];
            if urls.is_null() {
                return Ok(Vec::new());
            }

            let count: usize = msg_send![urls, count];
            let mut selected = Vec::with_capacity(count);
            for index in 0..count {
                let url: *mut AnyObject = msg_send![urls, objectAtIndex: index];
                if url.is_null() {
                    continue;
                }
                let path_ptr: *const std::ffi::c_char = msg_send![url, fileSystemRepresentation];
                if path_ptr.is_null() {
                    continue;
                }
                let path = CStr::from_ptr(path_ptr).to_string_lossy().into_owned();
                if !path.is_empty() {
                    selected.push(path);
                }
            }

            Ok(selected)
        })
    }

    pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn Error>> {
        let window = app
            .get_webview_window("main")
            .expect("Failed to get main window");

        {
            let app_state: tauri::State<'_, AppState> = app.state::<AppState>();
            let (about_mpv_version, about_ffmpeg_version) = {
                let mpv_guard = app_state
                    .mpv_player
                    .lock()
                    .map_err(|e| io::Error::other(e.to_string()))?;
                (
                    mpv_guard.get_property_string("mpv-version").ok(),
                    mpv_guard.get_property_string("ffmpeg-version").ok(),
                )
            };

            configure_about_menu(app, about_mpv_version, about_ffmpeg_version)
                .map_err(io::Error::other)?;
        }

        register_media_remote(&app.handle());
        register_pip_event_callback(&app.handle());

        unsafe {
            let app_handle = app.handle().clone();
            let window_label = window.label().to_string();

            let center = NSNotificationCenter::defaultCenter();

            let enter_block = RcBlock::new(move |_notif: std::ptr::NonNull<NSNotification>| {
                let _ = app_handle.emit_to(&window_label, "fullscreen-will-change", "enter");
            });

            let enter_observer = center.addObserverForName_object_queue_usingBlock(
                Some(&NSWindowWillEnterFullScreenNotification),
                None,
                None,
                &enter_block,
            );
            let enter_observer_raw = Retained::into_raw(enter_observer) as *mut AnyObject;
            let enter_block_raw = RcBlock::into_raw(enter_block) as *mut c_void;

            let app_handle = app.handle().clone();
            let window_label = window.label().to_string();

            let center = NSNotificationCenter::defaultCenter();

            let exit_block = RcBlock::new(move |_notif: std::ptr::NonNull<NSNotification>| {
                let _ = app_handle.emit_to(&window_label, "fullscreen-will-change", "exit");
            });

            let exit_observer = center.addObserverForName_object_queue_usingBlock(
                Some(&NSWindowWillExitFullScreenNotification),
                None,
                None,
                &exit_block,
            );
            let exit_observer_raw = Retained::into_raw(exit_observer) as *mut AnyObject;
            let exit_block_raw = RcBlock::into_raw(exit_block) as *mut c_void;

            let app_state: tauri::State<'_, PlatformState> = app.state::<PlatformState>();
            let fullscreen_observers = &app_state.fullscreen_observers;
            if let Ok(mut obs) = fullscreen_observers.lock() {
                *obs = Some(FullscreenObserversRaw {
                    enter_observer: enter_observer_raw as usize,
                    exit_observer: exit_observer_raw as usize,
                    enter_block: enter_block_raw as usize,
                    exit_block: exit_block_raw as usize,
                });
            };
        }

        Ok(())
    }

    pub(crate) fn cleanup_on_window_close(app_handle: &tauri::AppHandle, state: &PlatformState) {
        let _ = clear_now_playing_cache(app_handle);
        unregister_pip_event_callback(app_handle);
        cleanup_macos_resources(state);
    }
}

#[cfg(target_os = "macos")]
pub(crate) use imp::{
    apply_now_playing_info, apply_now_playing_status_async, apply_window_appearance,
    cleanup_on_window_close, clear_now_playing_cache, clear_now_playing_info,
    is_native_pip_enabled, pick_media_paths_native, set_native_pip_enabled,
    set_window_controls_visible, set_window_vibrancy_visible, setup,
    sync_mpv_metal_layer_geometry, update_native_pip_state, PlatformState,
};

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub(crate) struct PlatformState;
