use crate::store::{json_io, storage_paths};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONNECTIONS_FILE_NAME: &str = "network_connections.json";

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConnectionRecord {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub default_path: String,
}

fn connections_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = storage_paths::app_data_dir(app)?;
    Ok(data_dir.join(CONNECTIONS_FILE_NAME))
}

fn normalize_connection(
    connection: &NetworkConnectionRecord,
) -> Result<NetworkConnectionRecord, String> {
    let id = connection.id.trim();
    if id.is_empty() {
        return Err("Connection id is required".into());
    }

    let label = connection.label.trim();
    if label.is_empty() {
        return Err("Connection name is required".into());
    }

    let base_url = connection.base_url.trim();
    if base_url.is_empty() {
        return Err("Server URL is required".into());
    }

    let protocol = if connection.protocol.trim().is_empty() {
        "webdav".to_string()
    } else {
        connection.protocol.trim().to_string()
    };

    let is_dlna =
        protocol.eq_ignore_ascii_case("http-dlna") || protocol.eq_ignore_ascii_case("dlna");
    let is_http_based = is_dlna || protocol.eq_ignore_ascii_case("webdav");
    // Keep the stored URL parseable so browsing, playback and the connection list all agree,
    // even when the user typed `host:port/path` without a scheme.
    let base_url = if is_http_based {
        crate::network::protocols::normalize_http_base_url(base_url)
    } else {
        base_url.to_string()
    };

    let default_path = {
        let value = connection.default_path.trim();
        if is_dlna {
            let object_id = value.trim_start_matches('/');
            if object_id.is_empty() {
                "0".to_string()
            } else {
                object_id.to_string()
            }
        } else if value.is_empty() {
            "/".to_string()
        } else if value.starts_with('/') {
            value.to_string()
        } else {
            format!("/{}", value)
        }
    };

    Ok(NetworkConnectionRecord {
        id: id.to_string(),
        label: label.to_string(),
        protocol,
        base_url,
        username: connection.username.trim().to_string(),
        password: connection.password.clone(),
        default_path,
    })
}

pub fn list_network_connections(
    app: &tauri::AppHandle,
) -> Result<Vec<NetworkConnectionRecord>, String> {
    let path = connections_file_path(app)?;
    json_io::read_json_or_default(&path)
}

pub fn save_network_connection(
    app: &tauri::AppHandle,
    connection: NetworkConnectionRecord,
) -> Result<Vec<NetworkConnectionRecord>, String> {
    let path = connections_file_path(app)?;
    let normalized = normalize_connection(&connection)?;
    let mut connections: Vec<NetworkConnectionRecord> = json_io::read_json_or_default(&path)?;

    if let Some(index) = connections.iter().position(|item| item.id == normalized.id) {
        connections[index] = normalized;
    } else {
        connections.push(normalized);
    }

    json_io::write_json(&path, &connections)?;
    Ok(connections)
}

pub fn delete_network_connection(
    app: &tauri::AppHandle,
    connection_id: &str,
) -> Result<Vec<NetworkConnectionRecord>, String> {
    let path = connections_file_path(app)?;
    let mut connections: Vec<NetworkConnectionRecord> = json_io::read_json_or_default(&path)?;
    connections.retain(|item| item.id != connection_id);
    json_io::write_json(&path, &connections)?;
    Ok(connections)
}

pub fn find_network_connection(
    app: &tauri::AppHandle,
    connection_id: &str,
) -> Result<NetworkConnectionRecord, String> {
    let connection = list_network_connections(app)?
        .into_iter()
        .find(|item| item.id == connection_id);
    connection.ok_or_else(|| format!("Connection {} not found", connection_id))
}

pub fn clear_network_connections(app: &tauri::AppHandle) -> Result<(), String> {
    let path = connections_file_path(app)?;
    let empty_connections: Vec<NetworkConnectionRecord> = Vec::new();
    json_io::write_json(&path, &empty_connections)
}

#[cfg(test)]
mod tests {
    use super::{normalize_connection, NetworkConnectionRecord};

    fn record(protocol: &str, base_url: &str) -> NetworkConnectionRecord {
        NetworkConnectionRecord {
            id: "connection".to_string(),
            label: "Server".to_string(),
            protocol: protocol.to_string(),
            base_url: base_url.to_string(),
            username: String::new(),
            password: String::new(),
            default_path: "/".to_string(),
        }
    }

    #[test]
    fn adds_the_default_scheme_to_http_based_urls() {
        let webdav = normalize_connection(&record("webdav", " 192.168.31.25:5244/dav ")).unwrap();
        assert_eq!(webdav.base_url, "http://192.168.31.25:5244/dav");

        let dlna = normalize_connection(&record("http-dlna", "nas:8200/MediaServer")).unwrap();
        assert_eq!(dlna.base_url, "http://nas:8200/MediaServer");
    }

    #[test]
    fn leaves_other_protocols_untouched() {
        let smb = normalize_connection(&record("smb", "smb://nas/media")).unwrap();
        assert_eq!(smb.base_url, "smb://nas/media");

        let ftp = normalize_connection(&record("ftp", "ftp://nas:2121")).unwrap();
        assert_eq!(ftp.base_url, "ftp://nas:2121");
    }
}
