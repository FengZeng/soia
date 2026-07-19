use crate::core::navigation::NavigationState;
use crate::AppState;
use tauri::Manager;

#[tauri::command]
pub(crate) fn sync_navigation_state(
    state: tauri::State<'_, AppState>,
    payload: NavigationState,
) {
    state.navigation_service.sync_state(payload);
}

/// Execute a navigation action (previous/next) entirely within Core.
/// Resolves the target path using playlist-first strategy with directory-adjacency
/// fallback, then loads the resolved source.
pub(crate) async fn execute_core_navigation(
    app: &tauri::AppHandle,
    direction: i32,
) -> Result<(), String> {
    let state: tauri::State<'_, AppState> = app.state();

    let current_key = state
        .current_playback_key
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "no current playback source".to_string())?;

    if current_key.trim().is_empty() {
        return Err("no current playback source".to_string());
    }

    // Step 1: Try playlist resolution (synchronous)
    let playlist_result = state.navigation_service.resolve_playlist_path(&current_key, direction);

    // Step 2: If playlist resolution succeeded, use that path. Otherwise, try directory adjacency.
    let (target_key, title) = if let Some(nav_result) = playlist_result {
        (nav_result.path, nav_result.title)
    } else {
        let adjacent_key =
            crate::playback_source::adjacency::resolve_adjacent_key(app, &current_key, direction)
                .await?;
        match adjacent_key {
            Some(key) => {
                let title = state.navigation_service.get_title_for_path(&key);
                (key, title)
            }
            None => return Err("no adjacent media found".to_string()),
        }
    };

    // Step 3: Load the resolved source
    let payload = crate::commands::playback::LoadPlaybackSourcePayload::new(target_key.clone(), title);
    let _result = crate::commands::playback::load_source(app, &state, payload).await?;
    Ok(())
}
