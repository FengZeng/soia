use crate::network::types::{
    BrowseNetworkPayload, DiscoverNetworkPayload, DiscoveredNetworkConnection, NetworkBrowseResult,
};
use crate::store::network_connection_store::NetworkConnectionRecord;

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
        &app,
        &connection,
        protocol,
        payload.path,
        &payload.mode,
    );
    crate::network::service::browse_connection(&app, &connection, &path, protocol).await
}
