#[path = "remote/assets.rs"]
mod assets;
#[path = "remote/auth.rs"]
mod auth;
#[path = "remote/routes.rs"]
mod routes;
#[path = "remote/server.rs"]
mod server;
#[path = "remote/state.rs"]
mod state;
#[path = "remote/websocket.rs"]
mod websocket;

pub(crate) use server::start;

#[tauri::command]
pub(crate) fn get_remote_control_info() -> Result<state::RemoteControlInfo, String> {
    state::get_remote_control_info()
}

#[tauri::command]
pub(crate) fn get_remote_control_status() -> Result<state::RemoteControlStatus, String> {
    state::get_remote_control_status()
}

#[tauri::command]
pub(crate) fn set_remote_control_enabled(
    enabled: bool,
) -> Result<state::RemoteControlStatus, String> {
    state::set_remote_control_enabled(enabled)
}

#[tauri::command]
pub(crate) fn disconnect_remote_control_devices() -> Result<state::RemoteControlStatus, String> {
    state::disconnect_remote_control_devices()
}
