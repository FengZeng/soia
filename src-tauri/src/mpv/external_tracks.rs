use super::event_loop::{read_tracks_for_restore, restore_track_preferences, set_track_id};
use super::ffi::{mpv_abort_async_command, mpv_command_async};
use crate::core::external_tracks::ExternalTrackRequest;
use crate::AppState;
use std::collections::BTreeMap;
use std::ffi::{c_void, CString};
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub(super) struct ExternalTrackLoader {
    next_id: u64,
    pending: BTreeMap<u64, ExternalTrackRequest>,
}

impl ExternalTrackLoader {
    fn cancel_where(&mut self, mut cancel: impl FnMut(&ExternalTrackRequest) -> bool, mut abort: impl FnMut(u64)) {
        let ids: Vec<_> = self.pending.iter().filter(|(_, request)| cancel(request)).map(|(id, _)| *id).collect();
        for id in ids {
            abort(id);
            if let Some(request) = self.pending.remove(&id) {
                request.finish(Ok(false));
            }
        }
    }

    pub(super) unsafe fn cancel_all(&mut self, client: *mut c_void) {
        self.cancel_where(|_| true, |id| mpv_abort_async_command(client, id));
    }

    pub(super) unsafe fn pump(&mut self, app: &AppHandle, client: *mut c_void, playback: &Option<(u64, String)>) {
        let state = app.state::<AppState>();
        let generation = state.playback_load_coordinator.current();
        let active = |request: &ExternalTrackRequest| {
            request.generation == generation
                && playback.as_ref().is_some_and(|(loaded, key)| *loaded == generation && *key == request.playback_key)
                && state.track_selection.is_current(generation, &request.playback_key)
        };
        self.cancel_where(|request| !active(request), |id| mpv_abort_async_command(client, id));
        for request in state.track_selection.take_external_requests() {
            if !active(&request) {
                request.finish(Ok(false));
                continue;
            }
            if self.pending.values().any(|pending| pending.generation == request.generation
                && pending.kind == request.kind && pending.source == request.source)
            {
                request.finish(Ok(false));
                continue;
            }
            let tracks = read_tracks_for_restore(client);
            if state.track_selection.external_track_id(generation, request.kind, &request.source, &tracks).is_some() {
                restore_track_preferences(app, client, generation, &tracks);
                request.finish(Ok(false));
                continue;
            }
            let command = if request.kind == crate::store::track_preferences::TrackKind::Audio { "audio-add" } else { "sub-add" };
            // Always attach without selecting. Selection happens only after the
            // reply, with fresh generation and manual-choice checks.
            let args = [command, request.url.as_str(), "auto", request.title.as_str()];
            let strings: Result<Vec<_>, _> = args.iter().map(|arg| CString::new(*arg)).collect();
            let Ok(strings) = strings else {
                request.finish(Err("external track arguments contain a null byte".into()));
                continue;
            };
            let mut raw: Vec<_> = strings.iter().map(|arg| arg.as_ptr()).collect();
            raw.push(std::ptr::null());
            self.next_id = self.next_id.wrapping_add(1);
            let Some(result) = state.playback_load_coordinator.execute_if_current(generation, || {
                mpv_command_async(client, self.next_id, raw.as_ptr())
            }) else {
                request.finish(Ok(false));
                continue;
            };
            if result < 0 {
                request.finish(Err(format!("failed to queue external track: mpv error {result}")));
            } else {
                self.pending.insert(self.next_id, request);
            }
        }
    }

    pub(super) unsafe fn complete(&mut self, app: &AppHandle, client: *mut c_void, id: u64, error: i32, playback: &Option<(u64, String)>) {
        let Some(request) = self.pending.remove(&id) else { return };
        let state = app.state::<AppState>();
        let generation = state.playback_load_coordinator.current();
        if request.generation != generation
            || !playback.as_ref().is_some_and(|(loaded, key)| *loaded == generation && *key == request.playback_key)
            || !state.track_selection.is_current(generation, &request.playback_key)
        {
            request.finish(Ok(false));
            return;
        }
        if error < 0 {
            request.finish(Err(format!("failed to load external track: mpv error {error}")));
            return;
        }
        let tracks = read_tracks_for_restore(client);
        if let Some(track_id) = state.track_selection.external_track_id(generation, request.kind, &request.source, &tracks) {
            state.track_selection.select_added_track(generation, request.kind, track_id, request.select, || {
                set_track_id(client, request.kind, track_id);
            });
        }
        restore_track_preferences(app, client, generation, &tracks);
        request.finish(Ok(true));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpv::ffi::{
        mpv_command, mpv_create, mpv_event_id, mpv_format, mpv_free,
        mpv_get_property_string, mpv_initialize, mpv_observe_property,
        mpv_set_option_string, mpv_terminate_destroy, mpv_wait_event, SoiaStreamCbInfo, SoiaStreamOpenRoFn,
    };
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int};
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    fn request(generation: u64) -> (ExternalTrackRequest, tokio::sync::oneshot::Receiver<Result<bool, String>>) {
        let (completion, receiver) = tokio::sync::oneshot::channel();
        (ExternalTrackRequest {
            generation, playback_key: "video".into(), kind: crate::store::track_preferences::TrackKind::Subtitle,
            source: "subtitle.ass".into(), url: "subtitle.ass".into(), title: String::new(), select: true, completion,
        }, receiver)
    }

    #[test]
    fn replacing_playback_aborts_pending_requests_and_releases_waiters() {
        let mut loader = ExternalTrackLoader::default();
        let (old, mut old_result) = request(1);
        let (current, mut current_result) = request(2);
        loader.pending.insert(10, old);
        loader.pending.insert(11, current);
        let mut aborted = Vec::new();
        loader.cancel_where(|request| request.generation != 2, |id| aborted.push(id));
        assert_eq!(aborted, vec![10]);
        assert_eq!(old_result.try_recv().unwrap().unwrap(), false);
        assert!(current_result.try_recv().is_err());
        assert!(loader.pending.remove(&10).is_none());
        loader.cancel_where(|_| true, |_| {});
        assert_eq!(current_result.try_recv().unwrap().unwrap(), false);
    }

    unsafe extern "C" {
        fn mpv_stream_cb_add_ro(client: *mut c_void, protocol: *const c_char, userdata: *mut c_void, open: Option<SoiaStreamOpenRoFn>) -> c_int;
    }

    #[derive(Default)]
    struct SlowStream {
        state: Mutex<(bool, bool, bool)>, // read started, cancelled, finished
        changed: Condvar,
    }

    unsafe extern "C" fn open_slow_stream(userdata: *mut c_void, _uri: *mut c_char, info: *mut SoiaStreamCbInfo) -> c_int {
        *info = SoiaStreamCbInfo {
            cookie: userdata, read_fn: Some(read_slow_stream), seek_fn: None, size_fn: None,
            close_fn: Some(close_slow_stream), cancel_fn: Some(cancel_slow_stream),
        };
        0
    }

    unsafe extern "C" fn read_slow_stream(cookie: *mut c_void, _buffer: *mut c_char, _size: u64) -> i64 {
        let stream = &*cookie.cast::<SlowStream>();
        let mut state = stream.state.lock().unwrap_or_else(|p| p.into_inner());
        state.0 = true;
        stream.changed.notify_all();
        let (mut state, _) = stream.changed.wait_timeout_while(state, Duration::from_secs(5), |state| !state.1)
            .unwrap_or_else(|p| p.into_inner());
        state.2 = true;
        -1
    }

    unsafe extern "C" fn cancel_slow_stream(cookie: *mut c_void) {
        let stream = &*cookie.cast::<SlowStream>();
        stream.state.lock().unwrap_or_else(|p| p.into_inner()).1 = true;
        stream.changed.notify_all();
    }

    unsafe extern "C" fn close_slow_stream(_cookie: *mut c_void) {}

    struct HeadlessPlayer {
        client: *mut c_void,
        media: std::path::PathBuf,
        slow: Box<SlowStream>,
    }

    impl Drop for HeadlessPlayer {
        fn drop(&mut self) {
            unsafe { mpv_terminate_destroy(self.client); }
            let _ = std::fs::remove_file(&self.media);
        }
    }

    unsafe fn wait_for(client: *mut c_void, mut matches: impl FnMut(&crate::mpv::ffi::MpvEvent) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let event = mpv_wait_event(client, 0.02);
            if !event.is_null() && matches(&*event) { return true; }
        }
        false
    }

    unsafe fn command(client: *mut c_void, args: &[&str], async_id: Option<u64>) -> i32 {
        let strings: Vec<_> = args.iter().map(|arg| CString::new(*arg).unwrap()).collect();
        let mut raw: Vec<_> = strings.iter().map(|arg| arg.as_ptr()).collect();
        raw.push(std::ptr::null());
        match async_id {
            Some(id) => mpv_command_async(client, id, raw.as_ptr()),
            None => mpv_command(client, raw.as_ptr()),
        }
    }

    #[test]
    fn slow_external_source_keeps_mpv_events_responsive_and_can_be_aborted() {
        let media = std::env::temp_dir().join(format!("soia-track-test-{}.wav", uuid::Uuid::new_v4()));
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&16036u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        for value in [1u16, 1] { wav.extend_from_slice(&value.to_le_bytes()); }
        for value in [8000u32, 16000] { wav.extend_from_slice(&value.to_le_bytes()); }
        for value in [2u16, 16] { wav.extend_from_slice(&value.to_le_bytes()); }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.resize(16044, 0);
        std::fs::write(&media, wav).unwrap();

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        crate::mpv::ffi::ensure_numeric_locale_for_mpv();
        unsafe {
            let client = mpv_create();
            assert!(!client.is_null());
            let player = HeadlessPlayer { client, media, slow: Box::default() };
            for (name, value) in [("config", "no"), ("vo", "null"), ("ao", "null"), ("pause", "yes"), ("idle", "yes"), ("terminal", "no")] {
                assert!(mpv_set_option_string(client, CString::new(name).unwrap().as_ptr(), CString::new(value).unwrap().as_ptr()) >= 0);
            }
            assert!(mpv_initialize(client) >= 0);
            assert!(mpv_stream_cb_add_ro(client, CString::new("soia-test-slow").unwrap().as_ptr(),
                (&*player.slow as *const SlowStream).cast_mut().cast(), Some(open_slow_stream)) >= 0);
            assert!(command(client, &["loadfile", player.media.to_str().unwrap()], None) >= 0);
            assert!(wait_for(client, |event| matches!(event.event_id, mpv_event_id::MPV_EVENT_FILE_LOADED)));
            let started = Instant::now();
            assert!(command(client, &["sub-add", "soia-test-slow://subtitle", "auto"], Some(91)) >= 0);
            assert!(started.elapsed() < Duration::from_secs(1), "queuing waited for external I/O");
            let read_started = {
                let guard = player.slow.state.lock().unwrap();
                let (guard, _) = player.slow.changed.wait_timeout_while(guard, Duration::from_secs(3), |state| !state.0).unwrap();
                guard.0
            };
            assert!(read_started, "external read did not start");

            let volume = CString::new("volume").unwrap();
            let response_started = Instant::now();
            assert!(mpv_observe_property(client, 77, volume.as_ptr(), mpv_format::MPV_FORMAT_DOUBLE) >= 0);
            assert!(command(client, &["set", "volume", "42"], None) >= 0);
            assert!(wait_for(client, |event| matches!(event.event_id, mpv_event_id::MPV_EVENT_PROPERTY_CHANGE) && event.reply_usrdata == 77));
            let value = mpv_get_property_string(client, volume.as_ptr());
            assert!(!value.is_null());
            let volume: f64 = CStr::from_ptr(value).to_str().unwrap().parse().unwrap();
            mpv_free(value.cast());
            assert_eq!(volume, 42.0);
            assert!(response_started.elapsed() < Duration::from_secs(1), "playback events waited for external I/O");
            let still_reading = !player.slow.state.lock().unwrap().2;
            assert!(still_reading, "source finished before responsiveness was checked");

            mpv_abort_async_command(client, 91);
            assert!(wait_for(client, |event| matches!(event.event_id, mpv_event_id::MPV_EVENT_COMMAND_REPLY) && event.reply_usrdata == 91 && event.error < 0));
        }
    }
}
