use serde::Serialize;

use super::display::{display_path_for_smb, display_path_for_webdav};
use super::key::{
    create_dlna_playback_key, create_smb_playback_key, create_webdav_playback_key,
    decode_url_component, parse_playback_source, PlaybackSource,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SmbPlaybackSourceResult {
    connection_id: String,
    file_path: String,
    playback_key: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub(crate) enum ResolvedPlaybackSourceResult {
    Local {
        playback_key: String,
        display_path: String,
        file_path: String,
    },
    Webdav {
        playback_key: String,
        display_path: String,
        connection_id: String,
        file_path: String,
    },
    Dlna {
        playback_key: String,
        display_path: String,
        connection_id: String,
        resource_url: String,
        parent_path: Option<String>,
    },
    Smb {
        playback_key: String,
        display_path: String,
        connection_id: String,
        file_path: String,
    },
    DirectSmb {
        playback_key: String,
        display_path: String,
        resource_url: String,
    },
}

impl ResolvedPlaybackSourceResult {
    pub(crate) fn playback_key(&self) -> &str {
        match self {
            Self::Local { playback_key, .. }
            | Self::Webdav { playback_key, .. }
            | Self::Dlna { playback_key, .. }
            | Self::Smb { playback_key, .. }
            | Self::DirectSmb { playback_key, .. } => playback_key,
        }
    }
}

fn is_smb_protocol(protocol: &str) -> bool {
    let normalized = protocol.trim().to_ascii_lowercase();
    normalized == "smb" || normalized == "samba"
}

fn url_host_key(url: &url::Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

fn decoded_url_path_segments(url: &url::Url) -> Vec<String> {
    url.path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(decode_url_component)
        .collect()
}

fn normalized_path_from_segments(segments: &[String]) -> String {
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

async fn resolve_matching_smb_playback_source(
    app: &tauri::AppHandle,
    url: &str,
) -> Result<Option<SmbPlaybackSourceResult>, String> {
    let parsed = match url::Url::parse(url.trim()) {
        Ok(value) if value.scheme().eq_ignore_ascii_case("smb") => value,
        _ => return Ok(None),
    };
    let Some(url_host) = url_host_key(&parsed) else {
        return Ok(None);
    };
    let url_segments = decoded_url_path_segments(&parsed);

    let connections = crate::store::network_connection_store::list_network_connections(&app)?;
    let mut best_match: Option<(String, String, usize)> = None;
    for connection in connections {
        if !is_smb_protocol(&connection.protocol) {
            continue;
        }
        let Ok(base_url) = url::Url::parse(connection.base_url.trim()) else {
            continue;
        };
        if !base_url.scheme().eq_ignore_ascii_case("smb") {
            continue;
        }
        if url_host_key(&base_url).as_deref() != Some(url_host.as_str()) {
            continue;
        }

        let base_segments = decoded_url_path_segments(&base_url);
        let is_prefix = base_segments
            .iter()
            .enumerate()
            .all(|(index, segment)| url_segments.get(index) == Some(segment));
        if !is_prefix {
            continue;
        }

        let score = base_segments.len();
        if best_match
            .as_ref()
            .map(|(_, _, best_score)| score > *best_score)
            .unwrap_or(true)
        {
            let file_path = normalized_path_from_segments(&url_segments[base_segments.len()..]);
            best_match = Some((connection.id, file_path, score));
        }
    }

    Ok(best_match.map(|(connection_id, file_path, _)| {
        let playback_key = create_smb_playback_key(&connection_id, &file_path);
        SmbPlaybackSourceResult {
            connection_id,
            file_path,
            playback_key,
        }
    }))
}

pub(crate) async fn resolve(
    app: &tauri::AppHandle,
    key_or_url: &str,
) -> Result<ResolvedPlaybackSourceResult, String> {
    let raw = key_or_url;
    let trimmed = raw.trim();
    match parse_playback_source(raw) {
        PlaybackSource::Local { path } => {
            let playback_key = path.clone();
            Ok(ResolvedPlaybackSourceResult::Local {
                display_path: path.clone(),
                file_path: path,
                playback_key,
            })
        }
        PlaybackSource::Webdav {
            connection_id,
            file_path,
        } => {
            let playback_key = create_webdav_playback_key(&connection_id, &file_path);
            Ok(ResolvedPlaybackSourceResult::Webdav {
                playback_key,
                display_path: display_path_for_webdav(&connection_id, &file_path),
                connection_id,
                file_path,
            })
        }
        PlaybackSource::Dlna {
            connection_id,
            resource_url,
            parent_path,
        } => {
            let playback_key =
                create_dlna_playback_key(&connection_id, &resource_url, parent_path.as_deref());
            Ok(ResolvedPlaybackSourceResult::Dlna {
                playback_key,
                display_path: resource_url.clone(),
                connection_id,
                resource_url,
                parent_path,
            })
        }
        PlaybackSource::Smb {
            connection_id: Some(connection_id),
            file_path: Some(file_path),
        } => {
            let playback_key = create_smb_playback_key(&connection_id, &file_path);
            Ok(ResolvedPlaybackSourceResult::Smb {
                playback_key,
                display_path: display_path_for_smb(Some(&connection_id), Some(&file_path)),
                connection_id,
                file_path,
            })
        }
        PlaybackSource::DirectSmbUrl => {
            if let Some(source) = resolve_matching_smb_playback_source(app, trimmed).await? {
                return Ok(ResolvedPlaybackSourceResult::Smb {
                    playback_key: source.playback_key,
                    display_path: display_path_for_smb(
                        Some(&source.connection_id),
                        Some(&source.file_path),
                    ),
                    connection_id: source.connection_id,
                    file_path: source.file_path,
                });
            }
            Ok(ResolvedPlaybackSourceResult::DirectSmb {
                playback_key: trimmed.to_string(),
                display_path: trimmed.to_string(),
                resource_url: trimmed.to_string(),
            })
        }
        PlaybackSource::Smb { .. } => Err("Invalid SMB playback source".to_string()),
    }
}

#[tauri::command]
pub(crate) async fn resolve_playback_source(
    app: tauri::AppHandle,
    key_or_url: String,
) -> Result<ResolvedPlaybackSourceResult, String> {
    resolve(&app, &key_or_url).await
}
