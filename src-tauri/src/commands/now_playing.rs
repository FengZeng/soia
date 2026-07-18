use crate::store::storage_paths;
use crate::{build_artwork_file_name, with_now_playing_mut, AppState};
use log::debug;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::{timeout, Duration};

const ARTWORK_CAPTURE_TIMEOUT_MS: u64 = 400;
static ARTWORK_CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn set_now_playing_metadata(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    title: String,
    duration: Option<f64>,
    artwork_path: Option<String>,
) -> Result<(), String> {
    with_now_playing_mut(&state, |now_playing| {
        now_playing.title = Some(title);
        now_playing.duration = duration;
        now_playing.artwork_path = artwork_path;
    })?;
    crate::platform::apply_now_playing_info(&app, &state)
}

#[tauri::command]
pub(crate) fn clear_now_playing(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    with_now_playing_mut(&state, |now_playing| {
        *now_playing = Default::default();
    })?;
    crate::platform::clear_now_playing_cache(&app)?;
    crate::platform::clear_now_playing_info(&app)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn capture_now_playing_artwork(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<Option<String>, String> {
    if url.is_empty() {
        return Ok(None);
    }

    let artwork_dir = storage_paths::thumbnails_dir(&app)?;
    let file_path = artwork_dir.join(build_artwork_file_name(&url));
    let file_path_str = file_path.to_string_lossy().to_string();

    if ARTWORK_CAPTURE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        debug!("now_playing artwork capture skipped: another capture is in flight");
        return Ok(None);
    }

    struct InFlightReset;
    impl Drop for InFlightReset {
        fn drop(&mut self) {
            ARTWORK_CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
        }
    }
    let _inflight_reset = InFlightReset;

    let mpv_player = state.mpv_player.clone();
    let file_path_for_task = file_path_str.clone();
    let capture_task = tauri::async_runtime::spawn_blocking(move || {
        let Ok(mpv_guard) = mpv_player.try_lock() else {
            debug!("now_playing artwork capture skipped: mpv lock busy");
            return false;
        };
        mpv_guard.command(&["screenshot-to-file", &file_path_for_task, "video"]) == 0
    });

    let capture_result = timeout(
        Duration::from_millis(ARTWORK_CAPTURE_TIMEOUT_MS),
        capture_task,
    )
    .await;

    let capture_ok = match capture_result {
        Ok(Ok(value)) => value,
        Ok(Err(err)) => {
            debug!("now_playing artwork capture join error: {err}");
            false
        }
        Err(_) => {
            debug!(
                "now_playing artwork capture timed out after {}ms",
                ARTWORK_CAPTURE_TIMEOUT_MS
            );
            false
        }
    };

    if capture_ok {
        debug!("now_playing artwork capture success: {}", file_path_str);
        Ok(Some(file_path_str))
    } else {
        debug!("now_playing artwork capture failed or skipped");
        Ok(None)
    }
}
