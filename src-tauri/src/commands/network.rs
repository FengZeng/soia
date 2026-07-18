use crate::network::types::{
    BrowseNetworkPayload, DiscoverNetworkPayload, DiscoveredNetworkConnection,
    LoadNetworkFilePayload, NetworkBrowseResult,
};
use crate::store::network_connection_store::NetworkConnectionRecord;
use crate::{
    with_mpv, AppState,
};

#[tauri::command]
pub(crate) fn list_network_connections(
    app: tauri::AppHandle,
) -> Result<Vec<NetworkConnectionRecord>, String> {
    crate::store::network_connection_store::list_network_connections(&app)
}

#[tauri::command]
pub(crate) async fn discover_network_connections(
    app: tauri::AppHandle,
    payload: Option<DiscoverNetworkPayload>,
) -> Result<Vec<DiscoveredNetworkConnection>, String> {
    crate::network::service::discover_connections(
        &app,
        payload.as_ref().and_then(|item| item.protocol.as_deref()),
        payload.as_ref().and_then(|item| item.timeout_secs),
        payload.as_ref().and_then(|item| item.scan_id.as_deref()),
    )
    .await
}

#[tauri::command]
pub(crate) fn save_network_connection(
    app: tauri::AppHandle,
    connection: NetworkConnectionRecord,
) -> Result<Vec<NetworkConnectionRecord>, String> {
    crate::store::network_connection_store::save_network_connection(&app, connection)
}

#[tauri::command]
pub(crate) fn delete_network_connection(
    app: tauri::AppHandle,
    connection_id: String,
) -> Result<Vec<NetworkConnectionRecord>, String> {
    crate::store::network_connection_store::delete_network_connection(&app, &connection_id)
}

#[tauri::command]
pub(crate) async fn browse_network_connection(
    app: tauri::AppHandle,
    payload: BrowseNetworkPayload,
) -> Result<NetworkBrowseResult, String> {
    crate::network::service::validate_mode(&payload.mode)?;

    let connection = crate::store::network_connection_store::find_network_connection(
        &app,
        &payload.connection_id,
    )?;
    let protocol = crate::network::service::resolve_protocol_with_hint(
        &connection,
        payload.protocol.as_deref(),
    )?;

    let path = crate::network::service::resolve_browse_path(
        &connection,
        protocol,
        payload.path,
        &payload.mode,
    );
    crate::network::service::browse_connection(&app, &connection, &path, protocol).await
}

#[tauri::command]
pub(crate) fn load_network_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    payload: LoadNetworkFilePayload,
) -> Result<(), String> {
    let connection = crate::store::network_connection_store::find_network_connection(
        &app,
        &payload.connection_id,
    )?;
    let mut playback_url = crate::network::service::resolve_network_playback_url(
        &connection,
        payload.protocol.as_deref(),
        &payload.file_path,
    )?;
    let protocol = payload
        .protocol
        .as_deref()
        .unwrap_or(&connection.protocol)
        .trim()
        .to_ascii_lowercase();
    playback_url = crate::mpv::prepare_network_stream_url(
        &protocol,
        &playback_url,
        &connection.username,
        &connection.password,
    )?;
    if let Some(rewritten) = crate::mpv::rewrite_network_stream_url(&protocol, &playback_url) {
        playback_url = rewritten;
    }
    let load_options = crate::core::playback_loading::PlaybackLoadOptions::from_optional(
        payload.resume_position,
        payload.auto_play,
        payload.playback_speed,
    )?;

    with_mpv(&state, |mpv_guard| {
        crate::core::playback_loading::load(
            mpv_guard,
            &playback_url,
            &[],
            load_options,
            crate::core::playback_loading::LoadCommandMode::Direct,
        )
    })?;

    Ok(())
}
