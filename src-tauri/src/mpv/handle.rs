use super::event_loop::mpv_event_loop;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::ffi::ensure_numeric_locale_for_mpv;
use super::ffi::{
    mpv_command, mpv_create, mpv_create_client, mpv_destroy, mpv_format, mpv_free,
    mpv_get_property_string, mpv_initialize, mpv_set_option, mpv_set_option_string,
    mpv_terminate_destroy, resolve_linked_library_path, soia_utils_create, soia_utils_destroy,
    soia_utils_render_context_update, soia_utils_render_target_resize,
    soia_utils_set_render_target_visible, soia_utils_uses_render_context, SoiaUtils,
};
use super::stream_https::HttpsStreamRegistry;
use crate::check_update::SoiaAuthToken;
use log::{info, warn};
use std::ffi::{c_void, CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::AppHandle;

#[cfg(target_os = "windows")]
const WALLPAPER_MODE_SETTING_LABEL: &str = "WALLPAPER_MODE";
#[cfg(target_os = "windows")]
const WALLPAPER_MODE_ENABLED_VALUE: &str = "Enable";

const MPV_INITIAL_OPTIONS: &[(&str, &str)] = &[
    ("ytdl", "no"),
    // ("target-prim", "bt.709"),
    // ("target-trc", "srgb"),
    // ("target-colorspace-hint", "yes"),
    // ("target-colorspace-hint-mode", "target"),
    // ("tone-mapping", "bt.2390"),
    // ("gamut-mapping-mode", "perceptual"),
    // ("hdr-compute-peak", "yes"),
];

pub struct MpvHandle {
    ctx: AtomicPtr<c_void>,
    is_playing: Arc<AtomicBool>,
    is_rendering: Arc<AtomicBool>,
    eof_reached: Arc<AtomicBool>,
    app_handle: AppHandle,
    event_loop_stop: Arc<AtomicBool>,
    event_loop_handle: Mutex<Option<JoinHandle<()>>>,
    render_loop_stop: Arc<AtomicBool>,
    render_loop_handle: Mutex<Option<JoinHandle<()>>>,
    soia_utils: AtomicPtr<SoiaUtils>,
    _https_stream_registry: Option<Box<HttpsStreamRegistry>>,
}

unsafe impl Send for MpvHandle {}
unsafe impl Sync for MpvHandle {}

fn release_mpv_context(ctx: &AtomicPtr<c_void>, terminate: bool) {
    let ctx = ctx.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if ctx.is_null() {
        return;
    }
    info!("Freeing MPV player handle...");
    unsafe {
        if terminate {
            mpv_terminate_destroy(ctx);
        } else {
            mpv_destroy(ctx);
        }
    }
}

fn set_mpv_initial_options(ctx: *mut c_void) {
    for (name, value) in MPV_INITIAL_OPTIONS {
        let Ok(c_name) = CString::new(*name) else {
            continue;
        };
        let Ok(c_value) = CString::new(*value) else {
            continue;
        };
        let result = unsafe { mpv_set_option_string(ctx, c_name.as_ptr(), c_value.as_ptr()) };
        if result < 0 {
            warn!("Failed to set initial mpv color option {name}={value}: {result}");
        }
    }
}

fn release_soia_utils(utils_ptr: *mut SoiaUtils) {
    if utils_ptr.is_null() {
        return;
    }
    // println!("Freeing SoiaUtils instance...");
    unsafe {
        soia_utils_destroy(utils_ptr);
    }
}

fn shutdown_mpv_and_soia(ctx: &AtomicPtr<c_void>, utils: &AtomicPtr<SoiaUtils>, terminate: bool) {
    // Acquire ownership once to avoid load-then-destroy races with concurrent shutdown paths.
    let utils_ptr = utils.swap(std::ptr::null_mut(), Ordering::AcqRel);
    let uses_render_context = if utils_ptr.is_null() {
        false
    } else {
        unsafe { soia_utils_uses_render_context(utils_ptr) != 0 }
    };

    if uses_render_context {
        release_soia_utils(utils_ptr);
        release_mpv_context(ctx, terminate);
    } else {
        release_mpv_context(ctx, terminate);
        release_soia_utils(utils_ptr);
    }
}

fn log_runtime_library_diagnostics() {
    let mpv_path = resolve_linked_library_path(mpv_create as *const c_void)
        .unwrap_or_else(|| "<unresolved>".to_string());
    let soia_utils_path = resolve_linked_library_path(soia_utils_create as *const c_void)
        .unwrap_or_else(|| "<unresolved>".to_string());
    info!("Runtime library path: libmpv => {}", mpv_path);
    info!("Runtime library path: libsoia_utils => {}", soia_utils_path);
}

#[cfg(target_os = "windows")]
fn resolve_wallpaper_mode_enabled(app_handle: &AppHandle) -> bool {
    crate::store::ui_state_store::load_setting_value(app_handle, WALLPAPER_MODE_SETTING_LABEL)
        .ok()
        .flatten()
        .map(|value| value.eq_ignore_ascii_case(WALLPAPER_MODE_ENABLED_VALUE))
        .unwrap_or(false)
}

impl MpvHandle {
    fn ctx_ptr(&self) -> *mut c_void {
        self.ctx.load(Ordering::Acquire)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn soia_utils_ptr(&self) -> *mut SoiaUtils {
        self.soia_utils.load(Ordering::Acquire)
    }

    pub(crate) fn new(
        window: *const c_void,
        display: Option<*const c_void>,
        app_handle: AppHandle,
        _auth_token: Option<SoiaAuthToken>,
    ) -> Result<Self, String> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        ensure_numeric_locale_for_mpv();
        log_runtime_library_diagnostics();

        let ctx = unsafe { mpv_create() };
        if ctx.is_null() {
            return Err(
                "Failed to create MPV context. Verify runtime libs and ensure LC_NUMERIC is set to C."
                    .to_string(),
            );
        }

        set_mpv_initial_options(ctx);

        let init_result = unsafe { mpv_initialize(ctx) };
        if init_result < 0 {
            unsafe { mpv_destroy(ctx) };
            return Err(format!(
                "Initialize MPV context failed (error code: {})",
                init_result
            ));
        }

        let mode: i32;
        cfg_if::cfg_if! {
            if #[cfg(target_os = "macos")] {
                let use_render_context = std::env::var("SOIA_MPV_RENDER_CONTEXT")
                    .ok()
                    .map(|value| {
                        let normalized = value.trim().to_ascii_lowercase();
                        normalized == "1" || normalized == "true" || normalized == "yes"
                    })
                    .unwrap_or(false);
                mode = if use_render_context { 2 } else { 0 };
            } else if #[cfg(target_os = "linux")] {
                let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
                mode = if session_type == "wayland" { 2 } else { 1 };
            } else if #[cfg(target_os = "windows")] {
                let use_render_context = std::env::var("SOIA_MPV_RENDER_CONTEXT")
                    .ok()
                    .map(|value| {
                        let normalized = value.trim().to_ascii_lowercase();
                        normalized == "1" || normalized == "true" || normalized == "yes"
                    })
                    .unwrap_or(false);
                mode = if resolve_wallpaper_mode_enabled(&app_handle) {
                    5
                } else if use_render_context {
                    2
                } else {
                    1
                };
            } else {
                mode = 0;
            }
        }

        let display_ptr = display.unwrap_or(std::ptr::null());
        #[cfg(target_os = "macos")]
        let auth_payload = _auth_token
            .as_ref()
            .map(|token| token.payload.clone())
            .or_else(|| std::env::var("SOIA_AUTH_PAYLOAD").ok())
            .and_then(|value| CString::new(value).ok());
        #[cfg(not(target_os = "macos"))]
        let auth_payload = CString::new("").ok();
        let auth_payload_ptr = auth_payload
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr());
        #[cfg(target_os = "macos")]
        let auth_signature_hex = _auth_token
            .as_ref()
            .map(|token| token.signature_hex.clone())
            .or_else(|| std::env::var("SOIA_AUTH_SIGNATURE_HEX").ok())
            .and_then(|value| CString::new(value).ok());
        #[cfg(not(target_os = "macos"))]
        let auth_signature_hex = CString::new("").ok();
        let auth_signature_hex_ptr = auth_signature_hex
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr());
        let soia_utils: *mut SoiaUtils = unsafe {
            soia_utils_create(
                ctx,
                window,
                display_ptr,
                mode,
                auth_payload_ptr,
                auth_signature_hex_ptr,
            )
        };
        if soia_utils.is_null() {
            unsafe { mpv_destroy(ctx) };
            return Err("Failed to create SoiaUtils instance".to_string());
        }

        if let Err(error) = super::stream_proxy::start(app_handle.clone()) {
            log::warn!("stream proxy: failed to start: {error}");
        }
        let mut https_stream_registry = Box::new(HttpsStreamRegistry::new(app_handle.clone()));
        if let Err(error) = https_stream_registry.register(soia_utils) {
            log::warn!("{error}");
        }

        let handle = MpvHandle {
            ctx: AtomicPtr::new(ctx),
            is_playing: Arc::new(AtomicBool::new(false)),
            is_rendering: Arc::new(AtomicBool::new(false)),
            eof_reached: Arc::new(AtomicBool::new(false)),
            soia_utils: AtomicPtr::new(soia_utils),
            _https_stream_registry: Some(https_stream_registry),
            app_handle,
            event_loop_stop: Arc::new(AtomicBool::new(false)),
            event_loop_handle: Mutex::new(None),
            render_loop_stop: Arc::new(AtomicBool::new(false)),
            render_loop_handle: Mutex::new(None),
        };

        handle.start_render_loop();
        Ok(handle)
    }

    pub fn create_client(&self, name: &str) -> Result<*mut c_void, String> {
        let c_name = CString::new(name).expect("Option name contains null byte");
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return Err("MPV context is not initialized".to_string());
        }
        let client_ctx = unsafe { mpv_create_client(ctx, c_name.as_ptr()) };
        if client_ctx.is_null() {
            Err("Failed to create MPV client handle".to_string())
        } else {
            Ok(client_ctx)
        }
    }

    pub fn should_ignore_resize(&self) -> bool {
        !self.is_rendering.load(Ordering::Acquire)
    }

    pub fn render_target_resize(&mut self, width: u32, height: u32) {
        let utils = self.soia_utils.load(Ordering::Acquire);
        if !utils.is_null() {
            unsafe {
                soia_utils_render_target_resize(utils, width, height);
            }
        }
    }

    pub fn set_render_target_visible(&self, visible: bool) {
        let utils = self.soia_utils.load(Ordering::Acquire);
        if !utils.is_null() {
            unsafe {
                soia_utils_set_render_target_visible(utils, if visible { 1 } else { 0 });
            }
        }
    }

    pub fn command(&self, args: &[&str]) -> c_int {
        let c_strings: Vec<CString> = args
            .iter()
            .map(|&s| CString::new(s).expect("Argument contains null byte"))
            .collect();

        let mut raw_args: Vec<*const c_char> =
            c_strings.iter().map(|c_str| c_str.as_ptr()).collect();
        raw_args.push(std::ptr::null());

        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe { mpv_command(ctx, raw_args.as_ptr()) }
    }

    pub fn set_option_int(&self, name: &str, value: i64) -> c_int {
        let c_name = CString::new(name).expect("Option name contains null byte");
        let mut val = value;
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe {
            mpv_set_option(
                ctx,
                c_name.as_ptr(),
                mpv_format::MPV_FORMAT_INT64,
                &mut val as *mut i64 as *mut c_void,
            )
        }
    }

    pub fn set_option_double(&self, name: &str, value: f64) -> c_int {
        let c_name = CString::new(name).expect("Option name contains null byte");
        let mut val = value;
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe {
            mpv_set_option(
                ctx,
                c_name.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                &mut val as *mut f64 as *mut c_void,
            )
        }
    }

    pub fn set_option_flag(&self, name: &str, value: bool) -> c_int {
        let c_name = CString::new(name).expect("Option name contains null byte");
        let mut val: c_int = if value { 1 } else { 0 };
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe {
            mpv_set_option(
                ctx,
                c_name.as_ptr(),
                mpv_format::MPV_FORMAT_FLAG,
                &mut val as *mut c_int as *mut c_void,
            )
        }
    }

    pub fn set_option_string(&self, name: &str, value: &str) -> i32 {
        let c_name = CString::new(name).expect("CString::new failed");
        let c_value = CString::new(value).expect("CString::new failed");
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe { mpv_set_option_string(ctx, c_name.as_ptr(), c_value.as_ptr()) as i32 }
    }

    pub fn get_property_string(&self, name: &str) -> Result<String, String> {
        let c_name = CString::new(name).map_err(|_| "Invalid property name".to_string())?;
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return Err("MPV context is not initialized".to_string());
        }

        unsafe {
            let value_ptr = mpv_get_property_string(ctx, c_name.as_ptr());
            if value_ptr.is_null() {
                return Err(format!("Failed to get MPV property: {name}"));
            }
            let value = CStr::from_ptr(value_ptr).to_string_lossy().into_owned();
            mpv_free(value_ptr as *mut c_void);
            Ok(value)
        }
    }

    pub fn start_event_listener(&self) {
        self.stop_event_listener();
        self.event_loop_stop.store(false, Ordering::SeqCst);
        self.eof_reached.store(false, Ordering::SeqCst);
        let app_handle_clone = self.app_handle.clone();
        let stop_flag = self.event_loop_stop.clone();
        let is_playing = self.is_playing.clone();
        let is_rendering = self.is_rendering.clone();
        let eof_reached = self.eof_reached.clone();
        let handle = std::thread::spawn(move || {
            mpv_event_loop(
                app_handle_clone,
                stop_flag,
                is_playing,
                is_rendering,
                eof_reached,
            )
        });
        if let Ok(mut guard) = self.event_loop_handle.lock() {
            *guard = Some(handle);
        }
    }

    pub fn stop_event_listener(&self) {
        self.event_loop_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.event_loop_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        self.eof_reached.store(false, Ordering::SeqCst);
    }

    pub fn restart_event_listener(&self) {
        self.start_event_listener();
    }

    pub fn eof_reached(&self) -> bool {
        self.eof_reached.load(Ordering::Acquire)
    }

    fn start_render_loop(&self) {
        self.stop_render_loop();
        let utils = self.soia_utils.load(Ordering::Acquire);
        if utils.is_null() {
            return;
        }

        let enabled = unsafe { soia_utils_uses_render_context(utils) != 0 };
        if !enabled {
            return;
        }

        self.render_loop_stop.store(false, Ordering::SeqCst);
        let stop_flag = self.render_loop_stop.clone();
        let utils_addr = utils as usize;
        let handle = std::thread::spawn(move || {
            let utils_ptr = utils_addr as *mut SoiaUtils;
            while !stop_flag.load(Ordering::Relaxed) {
                unsafe {
                    soia_utils_render_context_update(utils_ptr);
                }
                std::thread::sleep(Duration::from_millis(8));
            }
        });
        if let Ok(mut guard) = self.render_loop_handle.lock() {
            *guard = Some(handle);
        }
    }

    fn stop_render_loop(&self) {
        self.render_loop_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.render_loop_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn terminate(&self) {
        self.stop_event_listener();
        self.stop_render_loop();
        shutdown_mpv_and_soia(&self.ctx, &self.soia_utils, true);
    }
}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        self.stop_event_listener();
        self.stop_render_loop();
        shutdown_mpv_and_soia(&self.ctx, &self.soia_utils, false);
    }
}
