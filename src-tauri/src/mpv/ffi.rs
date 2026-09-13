use log::warn;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn ensure_numeric_locale_for_mpv() {
    // libmpv requires LC_NUMERIC to be "C" for mpv_create().
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, b"C\0".as_ptr().cast());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[repr(C)]
struct DlInfo {
    dli_fname: *const c_char,
    dli_fbase: *mut c_void,
    dli_sname: *const c_char,
    dli_saddr: *mut c_void,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe extern "C" {
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn resolve_linked_library_path(symbol: *const c_void) -> Option<String> {
    if symbol.is_null() {
        return None;
    }

    let mut info = DlInfo {
        dli_fname: std::ptr::null(),
        dli_fbase: std::ptr::null_mut(),
        dli_sname: std::ptr::null(),
        dli_saddr: std::ptr::null_mut(),
    };

    let result = unsafe { dladdr(symbol, &mut info) };
    if result == 0 || info.dli_fname.is_null() {
        return None;
    }

    let raw_path = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }
        .to_string_lossy()
        .into_owned();

    std::fs::canonicalize(&raw_path)
        .map(|path| path.to_string_lossy().into_owned())
        .ok()
        .or(Some(raw_path))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn resolve_linked_library_path(_symbol: *const c_void) -> Option<String> {
    None
}

#[repr(C)]
#[derive(Debug, PartialEq)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub(crate) enum mpv_format {
    MPV_FORMAT_NONE = 0,
    MPV_FORMAT_STRING = 1,
    MPV_FORMAT_OSD_STRING = 2,
    MPV_FORMAT_FLAG = 3,
    MPV_FORMAT_INT64 = 4,
    MPV_FORMAT_DOUBLE = 5,
    MPV_FORMAT_NODE = 6,
    MPV_FORMAT_NODE_ARRAY = 7,
    MPV_FORMAT_NODE_MAP = 8,
}

impl std::fmt::Display for mpv_format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            mpv_format::MPV_FORMAT_NONE => write!(f, "MPV_FORMAT_NONE"),
            mpv_format::MPV_FORMAT_STRING => write!(f, "MPV_FORMAT_STRING"),
            mpv_format::MPV_FORMAT_OSD_STRING => write!(f, "MPV_FORMAT_OSD_STRING"),
            mpv_format::MPV_FORMAT_FLAG => write!(f, "MPV_FORMAT_FLAG"),
            mpv_format::MPV_FORMAT_INT64 => write!(f, "MPV_FORMAT_INT64"),
            mpv_format::MPV_FORMAT_DOUBLE => write!(f, "MPV_FORMAT_DOUBLE"),
            mpv_format::MPV_FORMAT_NODE => write!(f, "MPV_FORMAT_NODE"),
            mpv_format::MPV_FORMAT_NODE_ARRAY => write!(f, "MPV_FORMAT_NODE_ARRAY"),
            mpv_format::MPV_FORMAT_NODE_MAP => write!(f, "MPV_FORMAT_NODE_MAP"),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub(crate) enum mpv_event_id {
    MPV_EVENT_NONE = 0,
    MPV_EVENT_SHUTDOWN = 1,
    MPV_EVENT_LOG_MESSAGE = 2,
    MPV_EVENT_GET_PROPERTY_REPLY = 3,
    MPV_EVENT_SET_PROPERTY_REPLY = 4,
    MPV_EVENT_COMMAND_REPLY = 5,
    MPV_EVENT_START_FILE = 6,
    MPV_EVENT_END_FILE = 7,
    MPV_EVENT_FILE_LOADED = 8,
    MPV_EVENT_IDLE = 11,
    MPV_EVENT_TICK = 14,
    MPV_EVENT_CLIENT_MESSAGE = 16,
    MPV_EVENT_VIDEO_RECONFIG = 17,
    MPV_EVENT_AUDIO_RECONFIG = 18,
    MPV_EVENT_SEEK = 20,
    MPV_EVENT_PLAYBACK_RESTART = 21,
    MPV_EVENT_PROPERTY_CHANGE = 22,
    MPV_EVENT_QUEUE_OVERFLOW = 24,
    MPV_EVENT_HOOK = 25,
}

impl From<i32> for mpv_event_id {
    fn from(id: i32) -> Self {
        match id {
            0 => mpv_event_id::MPV_EVENT_NONE,
            1 => mpv_event_id::MPV_EVENT_SHUTDOWN,
            2 => mpv_event_id::MPV_EVENT_LOG_MESSAGE,
            3 => mpv_event_id::MPV_EVENT_GET_PROPERTY_REPLY,
            4 => mpv_event_id::MPV_EVENT_SET_PROPERTY_REPLY,
            5 => mpv_event_id::MPV_EVENT_COMMAND_REPLY,
            6 => mpv_event_id::MPV_EVENT_START_FILE,
            7 => mpv_event_id::MPV_EVENT_END_FILE,
            8 => mpv_event_id::MPV_EVENT_FILE_LOADED,
            11 => mpv_event_id::MPV_EVENT_IDLE,
            14 => mpv_event_id::MPV_EVENT_TICK,
            16 => mpv_event_id::MPV_EVENT_CLIENT_MESSAGE,
            17 => mpv_event_id::MPV_EVENT_VIDEO_RECONFIG,
            18 => mpv_event_id::MPV_EVENT_AUDIO_RECONFIG,
            20 => mpv_event_id::MPV_EVENT_SEEK,
            21 => mpv_event_id::MPV_EVENT_PLAYBACK_RESTART,
            22 => mpv_event_id::MPV_EVENT_PROPERTY_CHANGE,
            24 => mpv_event_id::MPV_EVENT_QUEUE_OVERFLOW,
            25 => mpv_event_id::MPV_EVENT_HOOK,
            _ => {
                warn!("Received unknown MPV event ID: {}", id);
                mpv_event_id::MPV_EVENT_NONE
            }
        }
    }
}

#[repr(C)]
pub(crate) struct mpv_node {
    pub u: mpv_node_union,
    pub format: mpv_format,
}

#[repr(C)]
pub(crate) union mpv_node_union {
    pub string: *mut c_char,
    pub flag: i32,
    pub int64: i64,
    pub double: f64,
    pub list: *mut mpv_node_list,
}

#[repr(C)]
pub(crate) struct mpv_node_list {
    pub num: i32,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct MpvEventProperty {
    pub name: *const c_char,
    pub format: mpv_format,
    pub data: *mut c_void,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct MpvEvent {
    pub event_id: mpv_event_id,
    pub error: c_int,
    pub reply_usrdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct MpvEventEndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

unsafe extern "C" {
    pub(super) fn mpv_create() -> *mut c_void;
    pub(super) fn mpv_load_config_file(ctx: *mut c_void, filename: *const c_char) -> c_int;
    pub(super) fn mpv_initialize(ctx: *mut c_void) -> c_int;
    pub(super) fn mpv_command(ctx: *mut c_void, args: *const *const c_char) -> c_int;
    pub(super) fn mpv_set_option(
        ctx: *mut c_void,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub(super) fn mpv_set_option_string(
        ctx: *mut c_void,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    pub(super) fn mpv_destroy(ctx: *mut c_void);
    pub(super) fn mpv_terminate_destroy(ctx: *mut c_void);
    pub(super) fn mpv_create_client(ctx: *mut c_void, name: *const c_char) -> *mut c_void;
    pub(super) fn mpv_observe_property(
        ctx: *mut c_void,
        reply_usrdata: u64,
        name: *const c_char,
        format: mpv_format,
    ) -> c_int;
    pub(super) fn mpv_wait_event(ctx: *mut c_void, timeout: f64) -> *mut MpvEvent;
    pub(super) fn mpv_get_property_string(ctx: *mut c_void, name: *const c_char) -> *mut c_char;
    pub(super) fn mpv_free(data: *mut c_void);
}

#[repr(C)]
pub(crate) struct SoiaUtils {
    _private: [u8; 0],
}

pub(crate) type SoiaStreamReadFn =
    unsafe extern "C" fn(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64;
pub(crate) type SoiaStreamSeekFn = unsafe extern "C" fn(cookie: *mut c_void, offset: i64) -> i64;
pub(crate) type SoiaStreamSizeFn = unsafe extern "C" fn(cookie: *mut c_void) -> i64;
pub(crate) type SoiaStreamCloseFn = unsafe extern "C" fn(cookie: *mut c_void);
pub(crate) type SoiaStreamCancelFn = unsafe extern "C" fn(cookie: *mut c_void);

#[repr(C)]
pub(crate) struct SoiaStreamCbInfo {
    pub cookie: *mut c_void,
    pub read_fn: Option<SoiaStreamReadFn>,
    pub seek_fn: Option<SoiaStreamSeekFn>,
    pub size_fn: Option<SoiaStreamSizeFn>,
    pub close_fn: Option<SoiaStreamCloseFn>,
    pub cancel_fn: Option<SoiaStreamCancelFn>,
}

pub(crate) type SoiaStreamOpenRoFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    uri: *mut c_char,
    info: *mut SoiaStreamCbInfo,
) -> c_int;

type SoiaUtilsRegisterStreamProtocolFn = unsafe extern "C" fn(
    utils: *mut SoiaUtils,
    protocol: *const c_char,
    userdata: *mut c_void,
    open_fn: Option<SoiaStreamOpenRoFn>,
) -> c_int;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn resolve_soia_utils_register_stream_protocol() -> Option<SoiaUtilsRegisterStreamProtocolFn> {
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            b"soia_utils_register_stream_protocol\0".as_ptr().cast(),
        )
    };
    if symbol.is_null() {
        None
    } else {
        Some(unsafe {
            std::mem::transmute::<*mut c_void, SoiaUtilsRegisterStreamProtocolFn>(symbol)
        })
    }
}

#[cfg(target_os = "windows")]
fn resolve_soia_utils_register_stream_protocol() -> Option<SoiaUtilsRegisterStreamProtocolFn> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleA(name: *const c_char) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    let module = unsafe { GetModuleHandleA(b"soia_utils.dll\0".as_ptr().cast()) };
    if module.is_null() {
        return None;
    }
    let symbol = unsafe {
        GetProcAddress(
            module,
            b"soia_utils_register_stream_protocol\0".as_ptr().cast(),
        )
    };
    if symbol.is_null() {
        None
    } else {
        Some(unsafe {
            std::mem::transmute::<*mut c_void, SoiaUtilsRegisterStreamProtocolFn>(symbol)
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn resolve_soia_utils_register_stream_protocol() -> Option<SoiaUtilsRegisterStreamProtocolFn> {
    None
}

pub(crate) unsafe fn soia_utils_register_stream_protocol(
    utils: *mut SoiaUtils,
    protocol: *const c_char,
    userdata: *mut c_void,
    open_fn: Option<SoiaStreamOpenRoFn>,
) -> Option<c_int> {
    resolve_soia_utils_register_stream_protocol()
        .map(|register| unsafe { register(utils, protocol, userdata, open_fn) })
}

#[cfg(target_os = "android")]
#[link(name = "avutil")]
unsafe extern "C" {
    pub(crate) fn av_jni_set_java_vm(vm: *mut c_void, log_ctx: *mut c_void) -> c_int;
}

#[link(name = "soia_utils")]
unsafe extern "C" {
    pub(super) fn soia_utils_create(
        mpv_ctx: *mut c_void,
        window: *const c_void,
        display: *const c_void,
        mode: i32,
        auth_payload: *const c_char,
        auth_signature_hex: *const c_char,
    ) -> *mut SoiaUtils;
    pub(super) fn soia_utils_render_target_resize(utils: *mut SoiaUtils, width: u32, height: u32);
    pub(super) fn soia_utils_set_render_target_visible(utils: *mut SoiaUtils, visible: c_int);
    pub(super) fn soia_utils_render_context_update(utils: *mut SoiaUtils);
    pub(super) fn soia_utils_uses_render_context(utils: *mut SoiaUtils) -> c_int;
    pub(super) fn soia_utils_destroy(utils: *mut SoiaUtils);
}
