#[cfg(target_os = "windows")]
mod imp {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::sync::mpsc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use tauri::{Emitter, Manager};
    use tokio::time::sleep;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateSolidBrush, FillRect, GetDC, InvalidateRect, ReleaseDC, UpdateWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, SetClassLongPtrW, GCLP_HBRBACKGROUND,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::IsZoomed;

    const MAIN_WINDOW_LABEL: &str = "main";
    const WINDOW_STATE_POLL_INTERVAL: Duration = Duration::from_millis(10);
    const WINDOW_STATE_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);
    const PIP_DEFAULT_ASPECT: f64 = 16.0 / 9.0;
    const PIP_MIN_WIDTH: u32 = 300;
    const PIP_MAX_WIDTH: u32 = 720;
    const PIP_SCREEN_WIDTH_FACTOR: f64 = 0.30;
    const PIP_SCREEN_HEIGHT_FACTOR: f64 = 0.45;
    const PIP_MARGIN: i32 = 20;
    const LIGHT_BACKGROUND_COLOR: u32 = 0x00f6f6f6;
    const DARK_BACKGROUND_COLOR: u32 = 0x000f0f0f;
    // Win32 COLORREF uses 0x00BBGGRR, so CSS #1e2227 becomes 0x0027221e.
    const GRAPHITE_BACKGROUND_COLOR: u32 = 0x0027221e;
    static CURRENT_BACKGROUND_COLOR: AtomicU32 = AtomicU32::new(0);

    pub(crate) fn paint_native_window_background<W: HasWindowHandle>(
        window: &W,
        theme: Option<&str>,
        system_theme: tauri::Theme,
    ) -> Result<(), String> {
        let handle = window.window_handle().map_err(|error| error.to_string())?;
        let hwnd = match handle.as_raw() {
            RawWindowHandle::Win32(handle) => {
                handle.hwnd.get() as usize as *mut std::ffi::c_void
            }
            _ => return Err("Main window does not expose a Win32 window handle".into()),
        };

        let color = match theme.map(|value| value.trim().to_ascii_lowercase()) {
            Some(value) if value == "light" => LIGHT_BACKGROUND_COLOR,
            Some(value) if value == "dark" => DARK_BACKGROUND_COLOR,
            Some(value) if value == "graphite" => GRAPHITE_BACKGROUND_COLOR,
            Some(_) => system_background_color(system_theme),
            None => {
                let current = CURRENT_BACKGROUND_COLOR.load(Ordering::Acquire);
                if current == 0 {
                    system_background_color(system_theme)
                } else {
                    current
                }
            }
        };
        CURRENT_BACKGROUND_COLOR.store(color, Ordering::Release);

        let brush = background_brush(color);
        if brush.is_null() {
            return Err("Failed to create native window background brush".into());
        }

        let mut client_rect = windows_sys::Win32::Foundation::RECT::default();
        unsafe {
            // The class owns this process-lifetime brush after assignment.
            SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, brush as isize);
            if GetClientRect(hwnd, &mut client_rect) == 0 {
                return Err("Failed to query main window client area".into());
            }

            let dc = GetDC(hwnd);
            if dc.is_null() {
                return Err("Failed to acquire main window device context".into());
            }
            FillRect(dc, &client_rect, brush);
            ReleaseDC(hwnd, dc);

            InvalidateRect(hwnd, std::ptr::null(), 1);
            UpdateWindow(hwnd);
        }
        Ok(())
    }

    fn system_background_color(theme: tauri::Theme) -> u32 {
        match theme {
            tauri::Theme::Light => LIGHT_BACKGROUND_COLOR,
            _ => DARK_BACKGROUND_COLOR,
        }
    }

    fn background_brush(color: u32) -> *mut std::ffi::c_void {
        static LIGHT_BRUSH: OnceLock<usize> = OnceLock::new();
        static DARK_BRUSH: OnceLock<usize> = OnceLock::new();
        static GRAPHITE_BRUSH: OnceLock<usize> = OnceLock::new();

        let slot = match color {
            LIGHT_BACKGROUND_COLOR => &LIGHT_BRUSH,
            GRAPHITE_BACKGROUND_COLOR => &GRAPHITE_BRUSH,
            _ => &DARK_BRUSH,
        };
        *slot.get_or_init(|| unsafe { CreateSolidBrush(color) as usize })
            as *mut std::ffi::c_void
    }

    #[derive(Clone, Copy, Debug)]
    struct WindowSnapshot {
        inner_size: Option<tauri::PhysicalSize<u32>>,
        outer_position: Option<tauri::PhysicalPosition<i32>>,
        was_decorated: bool,
        was_resizable: bool,
        was_always_on_top: bool,
        was_fullscreen: bool,
        was_maximized: bool,
    }

    #[derive(Debug, Default)]
    struct WindowPipState {
        enabled: bool,
        snapshot: Option<WindowSnapshot>,
        video_size: Option<(u32, u32)>,
        expected_size: Option<(u32, u32)>,
    }

    fn pip_state() -> &'static Mutex<WindowPipState> {
        static WINDOW_PIP_STATE: OnceLock<Mutex<WindowPipState>> = OnceLock::new();
        WINDOW_PIP_STATE.get_or_init(|| Mutex::new(WindowPipState::default()))
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

    fn as_i32(value: u32) -> i32 {
        i32::try_from(value).unwrap_or(i32::MAX)
    }

    fn is_native_window_maximized(window: &tauri::WebviewWindow) -> Result<bool, String> {
        let handle = window.window_handle().map_err(|error| error.to_string())?;
        let hwnd = match handle.as_raw() {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as usize as *mut std::ffi::c_void,
            _ => return Err("Main window does not expose a Win32 window handle".into()),
        };
        Ok(unsafe { IsZoomed(hwnd) != 0 })
    }

    async fn wait_for_window_to_unmaximize(window: &tauri::WebviewWindow) -> Result<(), String> {
        let deadline = Instant::now() + WINDOW_STATE_SETTLE_TIMEOUT;
        let mut settled_samples = 0;
        loop {
            let native_maximized = is_native_window_maximized(window)?;
            let tauri_maximized = window.is_maximized().map_err(|error| error.to_string())?;
            if !native_maximized && !tauri_maximized {
                settled_samples += 1;
                if settled_samples >= 2 {
                    return Ok(());
                }
            } else {
                settled_samples = 0;
            }
            if Instant::now() >= deadline {
                return Err("Timed out waiting for the maximized window to restore".into());
            }
            sleep(WINDOW_STATE_POLL_INTERVAL).await;
        }
    }

    fn current_aspect_ratio() -> f64 {
        let Ok(guard) = pip_state().lock() else {
            return PIP_DEFAULT_ASPECT;
        };
        match guard.video_size {
            Some((w, h)) if w > 0 && h > 0 => (w as f64 / h as f64).max(0.2),
            _ => PIP_DEFAULT_ASPECT,
        }
    }

    fn monitor_work_area(
        window: &tauri::WebviewWindow,
    ) -> Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)> {
        let monitor = window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten())?;
        let work_area = *monitor.work_area();
        Some((work_area.position, work_area.size))
    }

    fn compute_pip_size(window: &tauri::WebviewWindow, ratio: f64) -> tauri::PhysicalSize<u32> {
        let fallback = tauri::PhysicalSize::new(480_u32, 270_u32);
        let Some((_, work_size)) = monitor_work_area(window) else {
            return fallback;
        };

        let max_allowed_width = work_size.width.max(1);
        let min_allowed_width = PIP_MIN_WIDTH.min(max_allowed_width);
        let preferred_width = ((max_allowed_width as f64) * PIP_SCREEN_WIDTH_FACTOR).round() as u32;
        let mut width =
            preferred_width.clamp(min_allowed_width, PIP_MAX_WIDTH.min(max_allowed_width));
        if width == 0 {
            width = max_allowed_width;
        }

        let mut height = ((width as f64) / ratio.max(0.2)).round() as u32;
        let max_allowed_height =
            ((work_size.height as f64) * PIP_SCREEN_HEIGHT_FACTOR).round() as u32;
        if max_allowed_height > 0 && height > max_allowed_height {
            height = max_allowed_height.max(1);
            width = ((height as f64) * ratio.max(0.2)).round() as u32;
        }

        tauri::PhysicalSize::new(width.max(1), height.max(1))
    }

    fn compute_aspect_locked_size(
        requested_w: u32,
        requested_h: u32,
        ratio: f64,
    ) -> tauri::PhysicalSize<u32> {
        let requested_w = requested_w.max(1);
        let requested_h = requested_h.max(1);
        let ratio = ratio.max(0.2);

        let height_from_width = ((requested_w as f64) / ratio).round().max(1.0) as u32;
        let width_from_height = ((requested_h as f64) * ratio).round().max(1.0) as u32;

        let delta_keep_width = (height_from_width as i64 - requested_h as i64).abs();
        let delta_keep_height = (width_from_height as i64 - requested_w as i64).abs();

        if delta_keep_width <= delta_keep_height {
            tauri::PhysicalSize::new(requested_w, height_from_width)
        } else {
            tauri::PhysicalSize::new(width_from_height, requested_h)
        }
    }

    fn compute_pip_position(
        window: &tauri::WebviewWindow,
        pip_size: tauri::PhysicalSize<u32>,
    ) -> Option<tauri::PhysicalPosition<i32>> {
        let (work_pos, work_size) = monitor_work_area(window)?;
        let right = work_pos.x.saturating_add(as_i32(work_size.width));
        let bottom = work_pos.y.saturating_add(as_i32(work_size.height));
        let x = right
            .saturating_sub(as_i32(pip_size.width))
            .saturating_sub(PIP_MARGIN)
            .max(work_pos.x);
        let y = bottom
            .saturating_sub(as_i32(pip_size.height))
            .saturating_sub(PIP_MARGIN)
            .max(work_pos.y);
        Some(tauri::PhysicalPosition::new(x, y))
    }

    fn capture_window_snapshot(window: &tauri::WebviewWindow) -> WindowSnapshot {
        WindowSnapshot {
            inner_size: window.inner_size().ok(),
            outer_position: window.outer_position().ok(),
            was_decorated: window.is_decorated().unwrap_or(true),
            was_resizable: window.is_resizable().unwrap_or(true),
            was_always_on_top: window.is_always_on_top().unwrap_or(false),
            was_fullscreen: window.is_fullscreen().unwrap_or(false),
            was_maximized: window.is_maximized().unwrap_or(false),
        }
    }

    fn apply_window_pip_mode(window: &tauri::WebviewWindow, ratio: f64) -> Result<(), String> {
        let snapshot = capture_window_snapshot(window);

        if snapshot.was_fullscreen {
            let _ = window.set_fullscreen(false);
        }
        if snapshot.was_maximized {
            let _ = window.unmaximize();
        }

        let pip_size = compute_pip_size(window, ratio);

        window.set_resizable(true).map_err(|e| e.to_string())?;
        window.set_decorations(false).map_err(|e| e.to_string())?;
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
        let _ = window.set_skip_taskbar(true);
        window.set_size(pip_size).map_err(|e| e.to_string())?;
        if let Some(position) = compute_pip_position(window, pip_size) {
            window.set_position(position).map_err(|e| e.to_string())?;
        }

        let mut guard = pip_state().lock().map_err(|e| e.to_string())?;
        guard.snapshot = Some(snapshot);
        guard.enabled = true;
        guard.expected_size = Some((pip_size.width, pip_size.height));
        Ok(())
    }

    fn restore_window_from_pip(window: &tauri::WebviewWindow) -> Result<(), String> {
        let snapshot = {
            let mut guard = pip_state().lock().map_err(|e| e.to_string())?;
            guard.enabled = false;
            guard.expected_size = None;
            guard.snapshot.take()
        };

        let _ = window.set_skip_taskbar(false);
        let default_snapshot = WindowSnapshot {
            inner_size: None,
            outer_position: None,
            was_decorated: false,
            was_resizable: true,
            was_always_on_top: false,
            was_fullscreen: false,
            was_maximized: false,
        };
        let restore = snapshot.unwrap_or(default_snapshot);

        window
            .set_always_on_top(restore.was_always_on_top)
            .map_err(|e| e.to_string())?;
        window
            .set_decorations(restore.was_decorated)
            .map_err(|e| e.to_string())?;
        window
            .set_resizable(restore.was_resizable)
            .map_err(|e| e.to_string())?;

        if let Some(size) = restore.inner_size {
            window.set_size(size).map_err(|e| e.to_string())?;
        }
        if let Some(position) = restore.outer_position {
            window.set_position(position).map_err(|e| e.to_string())?;
        }
        if restore.was_maximized {
            let _ = window.maximize();
        }
        if restore.was_fullscreen {
            let _ = window.set_fullscreen(true);
        }
        Ok(())
    }

    fn set_enabled_internal(app_handle: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
        let window = app_handle
            .get_webview_window(MAIN_WINDOW_LABEL)
            .ok_or_else(|| "Failed to resolve main window for Picture in Picture".to_string())?;

        let already_enabled = pip_state()
            .lock()
            .map_err(|e| e.to_string())
            .map(|guard| guard.enabled)?;
        if already_enabled == enabled {
            return Ok(());
        }

        if enabled {
            apply_window_pip_mode(&window, current_aspect_ratio())?;
        } else {
            restore_window_from_pip(&window)?;
        }

        let _ = app_handle.emit("native-pip-changed", enabled);
        Ok(())
    }

    pub(crate) fn is_native_pip_enabled(_app_handle: &tauri::AppHandle) -> bool {
        pip_state()
            .lock()
            .map(|guard| guard.enabled)
            .unwrap_or(false)
    }

    pub(crate) async fn prepare_window_for_fullscreen(
        window: tauri::WebviewWindow,
    ) -> Result<bool, String> {
        // Tauri updates its cached maximize flag from WM_SIZE, which can lag
        // behind the real Win32 state during a maximize/restore transition.
        let was_maximized =
            is_native_window_maximized(&window)? || window.is_maximized().unwrap_or(false);
        if !was_maximized {
            return Ok(false);
        }

        window.unmaximize().map_err(|error| error.to_string())?;
        if let Err(error) = wait_for_window_to_unmaximize(&window).await {
            let _ = window.maximize();
            return Err(error);
        }
        Ok(true)
    }

    pub(crate) fn set_native_pip_enabled(
        app_handle: &tauri::AppHandle,
        enabled: bool,
    ) -> Result<(), String> {
        let state_handle = app_handle.clone();
        let main_thread_handle = state_handle.clone();
        run_on_main_thread_sync(&main_thread_handle, move || {
            set_enabled_internal(&state_handle, enabled)
        })
    }

    pub(crate) fn update_native_pip_state(
        _app_handle: &tauri::AppHandle,
        _paused: bool,
        video_w: i64,
        video_h: i64,
    ) {
        if video_w <= 0 || video_h <= 0 {
            return;
        }
        if let Ok(mut guard) = pip_state().lock() {
            guard.video_size = Some((video_w as u32, video_h as u32));
        }
    }

    pub(crate) fn enforce_native_pip_aspect(
        window: &tauri::WebviewWindow,
        width: u32,
        height: u32,
    ) -> bool {
        if width == 0 || height == 0 {
            return false;
        }

        let current_ratio = current_aspect_ratio();
        let requested = (width, height);

        {
            let Ok(mut guard) = pip_state().lock() else {
                return false;
            };
            if !guard.enabled {
                guard.expected_size = None;
                return false;
            }
            if let Some((expected_w, expected_h)) = guard.expected_size {
                if expected_w == requested.0 && expected_h == requested.1 {
                    guard.expected_size = None;
                    return false;
                }
            }
        }

        let locked_size = compute_aspect_locked_size(requested.0, requested.1, current_ratio);
        if locked_size.width == requested.0 && locked_size.height == requested.1 {
            return false;
        }

        if window.set_size(locked_size).is_ok() {
            if let Ok(mut guard) = pip_state().lock() {
                guard.expected_size = Some((locked_size.width, locked_size.height));
            }
            return true;
        }

        false
    }
}

#[cfg(target_os = "windows")]
pub(crate) use imp::{
    enforce_native_pip_aspect, is_native_pip_enabled, paint_native_window_background,
    prepare_window_for_fullscreen, set_native_pip_enabled, update_native_pip_state,
};
