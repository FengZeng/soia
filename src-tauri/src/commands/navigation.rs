use crate::core::navigation::NavigationState;
use crate::AppState;

#[tauri::command]
pub(crate) fn sync_navigation_state(
    state: tauri::State<'_, AppState>,
    payload: NavigationState,
) {
    state.navigation_service.sync_state(payload);
}
