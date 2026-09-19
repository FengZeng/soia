#[cfg(target_os = "macos")]
use crate::mpv::SoiaUtils;
#[cfg(target_os = "macos")]
use std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "soia_utils")]
unsafe extern "C" {
    pub(crate) fn soia_sync_layer_geometry(ns_view: *mut c_void, utils: usize);
    pub(crate) fn soia_utils_set_render_target_hdr(utils: *mut SoiaUtils, enabled: i32);
    pub(crate) fn soia_utils_set_pip_enabled(utils: *mut SoiaUtils, enabled: i32) -> i32;
    pub(crate) fn soia_utils_update_pip_state_values(
        utils: *mut SoiaUtils,
        paused: i32,
        video_w: f64,
        video_h: f64,
        seek_step_seconds: f64,
    ) -> i32;
    pub(crate) fn soia_utils_is_pip_enabled(utils: *mut SoiaUtils) -> i32;
    pub(crate) fn soia_utils_set_pip_event_callback(
        utils: *mut SoiaUtils,
        ctx: *mut c_void,
        cb: Option<extern "C" fn(*mut c_void, i32)>,
    );
    pub(crate) fn soia_utils_apply_now_playing_info_values(
        utils: *mut SoiaUtils,
        title: *const i8,
        duration: f64,
        position: f64,
        is_playing: i32,
        artwork_path: *const i8,
    ) -> i32;
    pub(crate) fn soia_utils_apply_now_playing_status_values(
        utils: *mut SoiaUtils,
        title: *const i8,
        duration: f64,
        position: f64,
        is_playing: i32,
    ) -> i32;
    pub(crate) fn soia_utils_clear_now_playing_cache(utils: *mut SoiaUtils) -> i32;
    pub(crate) fn soia_utils_clear_now_playing_info(utils: *mut SoiaUtils) -> i32;
    pub(crate) fn soia_utils_register_media_remote(utils: *mut SoiaUtils) -> i32;
}
