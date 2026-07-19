use serde::{Deserialize, Serialize};

use super::key::{
    create_dlna_playback_key, create_smb_playback_key, create_webdav_playback_key,
    normalize_file_path, parse_playback_source, path_extension, path_parent,
    resolve_local_media_path, PlaybackSource,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdjacentPlaybackSourcePayload {
    playback_key: String,
    direction: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdjacentPlaybackSourceResult {
    playback_key: String,
}

fn is_media_path(path: &str) -> bool {
    crate::media_extensions::contains_extension(&path_extension(path))
}

fn sorted_file_entries(
    entries: Vec<crate::network::types::NetworkBrowseEntry>,
    protocol: crate::network::service::BrowseProtocol,
) -> Vec<crate::network::types::NetworkBrowseEntry> {
    let mut files = entries
        .into_iter()
        .filter(|entry| {
            entry.entry_type == "file"
                && (protocol == crate::network::service::BrowseProtocol::Dlna
                    || is_media_path(&entry.path))
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    files
}

fn adjacent_from_index<T>(items: &[T], current_index: usize, direction: i32) -> Option<&T> {
    let next_index = current_index as i32 + direction;
    if next_index < 0 || next_index >= items.len() as i32 {
        return None;
    }
    items.get(next_index as usize)
}

fn normalize_dlna_object_id(value: &str) -> String {
    value.trim().trim_start_matches('/').to_string()
}

fn resolve_adjacent_local(path: &str, direction: i32) -> Option<String> {
    let local_path = resolve_local_media_path(path)?;
    if !local_path.is_file() || !is_media_path(&local_path.to_string_lossy()) {
        return None;
    }
    let parent = local_path.parent()?;
    let mut files = std::fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry_path| entry_path.is_file() && is_media_path(&entry_path.to_string_lossy()))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let right_name = right
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_lowercase();
        left_name.cmp(&right_name)
    });
    let current_index = files.iter().position(|entry| entry == &local_path)?;
    adjacent_from_index(&files, current_index, direction)
        .map(|path| path.to_string_lossy().into_owned())
}

async fn resolve_adjacent_network(
    app: &tauri::AppHandle,
    connection_id: &str,
    protocol: crate::network::service::BrowseProtocol,
    current_path: &str,
    parent_path: &str,
    direction: i32,
) -> Result<Option<String>, String> {
    let connection =
        crate::store::network_connection_store::find_network_connection(app, connection_id)?;
    let result =
        crate::network::service::browse_connection(app, &connection, parent_path, protocol).await?;
    let files = sorted_file_entries(result.entries, protocol);
    let current_index = files
        .iter()
        .position(|entry| normalize_file_path(&entry.path) == normalize_file_path(current_path));
    let Some(current_index) = current_index else {
        return Ok(None);
    };
    let Some(next_entry) = adjacent_from_index(&files, current_index, direction) else {
        return Ok(None);
    };
    let playback_key = match protocol {
        crate::network::service::BrowseProtocol::Webdav => {
            create_webdav_playback_key(connection_id, &next_entry.path)
        }
        crate::network::service::BrowseProtocol::Dlna => {
            create_dlna_playback_key(connection_id, &next_entry.path, Some(parent_path))
        }
        crate::network::service::BrowseProtocol::Smb => {
            create_smb_playback_key(connection_id, &next_entry.path)
        }
    };
    Ok(Some(playback_key))
}

#[tauri::command]
pub(crate) async fn resolve_adjacent_playback_source(
    app: tauri::AppHandle,
    payload: AdjacentPlaybackSourcePayload,
) -> Result<Option<AdjacentPlaybackSourceResult>, String> {
    let direction = match payload.direction {
        value if value < 0 => -1,
        value if value > 0 => 1,
        _ => return Ok(None),
    };

    let playback_key = match parse_playback_source(&payload.playback_key) {
        PlaybackSource::Local { path } => resolve_adjacent_local(&path, direction),
        PlaybackSource::Webdav {
            connection_id,
            file_path,
        } => {
            let Some(parent_path) = path_parent(&file_path) else {
                return Ok(None);
            };
            resolve_adjacent_network(
                &app,
                &connection_id,
                crate::network::service::BrowseProtocol::Webdav,
                &file_path,
                &parent_path,
                direction,
            )
            .await?
        }
        PlaybackSource::Dlna {
            connection_id,
            resource_url,
            parent_path,
        } => {
            let Some(parent_path) = parent_path else {
                return Ok(None);
            };
            let parent_path = normalize_dlna_object_id(&parent_path);
            resolve_adjacent_network(
                &app,
                &connection_id,
                crate::network::service::BrowseProtocol::Dlna,
                &resource_url,
                &parent_path,
                direction,
            )
            .await?
        }
        PlaybackSource::Smb {
            connection_id: Some(connection_id),
            file_path: Some(file_path),
            ..
        } => {
            let Some(parent_path) = path_parent(&file_path) else {
                return Ok(None);
            };
            resolve_adjacent_network(
                &app,
                &connection_id,
                crate::network::service::BrowseProtocol::Smb,
                &file_path,
                &parent_path,
                direction,
            )
            .await?
        }
        PlaybackSource::Smb { .. } | PlaybackSource::DirectSmbUrl => None,
    };

    Ok(playback_key.map(|playback_key| AdjacentPlaybackSourceResult {
        playback_key,
    }))
}

/// Resolve the adjacent playback source key in the same directory.
/// Returns the playback key of the adjacent media file, or `None` if there is
/// no adjacent file in the given direction.
pub(crate) async fn resolve_adjacent_key(
    app: &tauri::AppHandle,
    current_key: &str,
    direction: i32,
) -> Result<Option<String>, String> {
    let direction = match direction {
        value if value < 0 => -1,
        value if value > 0 => 1,
        _ => return Ok(None),
    };

    let playback_key = match parse_playback_source(current_key) {
        PlaybackSource::Local { path } => resolve_adjacent_local(&path, direction),
        PlaybackSource::Webdav {
            connection_id,
            file_path,
        } => {
            let Some(parent_path) = path_parent(&file_path) else {
                return Ok(None);
            };
            resolve_adjacent_network(
                app,
                &connection_id,
                crate::network::service::BrowseProtocol::Webdav,
                &file_path,
                &parent_path,
                direction,
            )
            .await?
        }
        PlaybackSource::Dlna {
            connection_id,
            resource_url,
            parent_path,
        } => {
            let Some(parent_path) = parent_path else {
                return Ok(None);
            };
            let parent_path = normalize_dlna_object_id(&parent_path);
            resolve_adjacent_network(
                app,
                &connection_id,
                crate::network::service::BrowseProtocol::Dlna,
                &resource_url,
                &parent_path,
                direction,
            )
            .await?
        }
        PlaybackSource::Smb {
            connection_id: Some(connection_id),
            file_path: Some(file_path),
            ..
        } => {
            let Some(parent_path) = path_parent(&file_path) else {
                return Ok(None);
            };
            resolve_adjacent_network(
                app,
                &connection_id,
                crate::network::service::BrowseProtocol::Smb,
                &file_path,
                &parent_path,
                direction,
            )
            .await?
        }
        PlaybackSource::Smb { .. } | PlaybackSource::DirectSmbUrl => None,
    };

    Ok(playback_key)
}
