use serde::{Deserialize, Serialize};
use crate::playback_source::{
    parse_playback_source, path_extension, path_file_name, path_parent, path_stem,
    resolve_local_media_path, PlaybackSource,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExternalSubtitleMatchesPayload {
    playback_key: String,
    media_title: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExternalSubtitleMatch {
    path: String,
    name: String,
}

#[derive(Clone)]
struct SubtitleMatchEntry {
    name: String,
    path: String,
}

const SUBTITLE_FILE_EXTENSIONS: &[&str] = &[
    "srt", "ass", "ssa", "vtt", "sub", "idx", "sup", "smi", "smil", "lrc", "ttml", "dfxp",
];

const IGNORED_SUBTITLE_MATCH_TOKENS: &[&str] = &[
    "1080p", "2160p", "720p", "480p", "4k", "8k", "web", "webrip", "webdl", "web-dl", "dl",
    "bluray", "bdrip", "hdrip", "dvdrip", "hdtv", "x264", "x265", "h264", "h265", "hevc",
    "avc", "aac", "dts", "ddp", "atmos", "proper", "repack", "remux", "extended", "internal",
    "multi", "chs", "cht", "chi", "zho", "zh", "cn", "gb", "big5", "eng", "en", "jpn", "jp",
    "kor", "kr", "sc", "tc",
];

fn is_subtitle_file(path: &str) -> bool {
    let ext = path_extension(path);
    SUBTITLE_FILE_EXTENSIONS.iter().any(|item| *item == ext)
}

fn is_matching_subtitle_folder(media_file_name: &str, folder_name: &str) -> bool {
    let media_stem = normalize_match_text(&path_stem(media_file_name));
    let folder_name = normalize_match_text(folder_name);
    !media_stem.is_empty() && media_stem == folder_name
}

fn nested_subtitle_match_name(media_file_name: &str, subtitle_file_name: &str) -> String {
    format!("{} {}", path_stem(media_file_name), subtitle_file_name)
}

fn normalize_match_text(value: &str) -> String {
    let mut result = String::new();
    let mut previous_space = false;
    for ch in value.to_lowercase().chars() {
        let next = if ch == '\'' || ch == '"' {
            None
        } else if ch.is_alphanumeric() {
            Some(ch)
        } else {
            Some(' ')
        };
        if let Some(ch) = next {
            if ch == ' ' {
                if !previous_space {
                    result.push(ch);
                    previous_space = true;
                }
            } else {
                result.push(ch);
                previous_space = false;
            }
        }
    }
    result.trim().to_string()
}

fn tokenize_match_text(value: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for token in value.split_whitespace() {
        if IGNORED_SUBTITLE_MATCH_TOKENS.iter().any(|item| *item == token) {
            continue;
        }
        if !tokens.iter().any(|item| item == token) {
            tokens.push(token.to_string());
        }
    }
    tokens
}

fn extract_episode_keys(value: &str) -> Vec<String> {
    let compact = value
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>();
    let chars = compact.chars().collect::<Vec<_>>();
    let mut result: Vec<String> = Vec::new();
    for index in 0..chars.len() {
        if chars[index] != 's' {
            continue;
        }
        let mut cursor = index + 1;
        let season_start = cursor;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && cursor - season_start < 2 {
            cursor += 1;
        }
        if season_start == cursor || cursor >= chars.len() || chars[cursor] != 'e' {
            continue;
        }
        cursor += 1;
        let episode_start = cursor;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && cursor - episode_start < 3 {
            cursor += 1;
        }
        if episode_start == cursor {
            continue;
        }
        let key = chars[index..cursor].iter().collect::<String>();
        if !result.iter().any(|item| item == &key) {
            result.push(key);
        }
    }
    result
}

fn extract_year_keys(value: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for token in normalize_match_text(value).split_whitespace() {
        if token.len() != 4 || !token.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if let Ok(year) = token.parse::<u16>() {
            if (1900..=2099).contains(&year) && !result.iter().any(|item| item == token) {
                result.push(token.to_string());
            }
        }
    }
    result
}

fn has_boundary_prefix(value: &str, prefix: &str) -> bool {
    value == prefix || value.starts_with(&format!("{prefix} "))
}

fn subtitle_match_score(media_name: &str, subtitle_name: &str) -> f64 {
    let media_stem = path_stem(media_name);
    let subtitle_stem = path_stem(subtitle_name);
    let normalized_media = normalize_match_text(&media_stem);
    let normalized_subtitle = normalize_match_text(&subtitle_stem);
    if normalized_media.is_empty() || normalized_subtitle.is_empty() {
        return 0.0;
    }
    if normalized_media == normalized_subtitle {
        return 100.0;
    }

    let media_episode_keys = extract_episode_keys(&media_stem);
    let subtitle_episode_keys = extract_episode_keys(&subtitle_stem);
    if !media_episode_keys.is_empty()
        && !media_episode_keys
            .iter()
            .any(|key| subtitle_episode_keys.iter().any(|item| item == key))
    {
        return 0.0;
    }

    let media_year_keys = extract_year_keys(&media_stem);
    let subtitle_year_keys = extract_year_keys(&subtitle_stem);
    if !media_year_keys.is_empty()
        && !subtitle_year_keys.is_empty()
        && !media_year_keys
            .iter()
            .any(|key| subtitle_year_keys.iter().any(|item| item == key))
    {
        return 0.0;
    }

    if has_boundary_prefix(&normalized_subtitle, &normalized_media) {
        return 95.0;
    }
    if has_boundary_prefix(&normalized_media, &normalized_subtitle) {
        return 85.0;
    }

    let media_tokens = tokenize_match_text(&normalized_media);
    let subtitle_tokens = tokenize_match_text(&normalized_subtitle);
    if media_tokens.is_empty() || subtitle_tokens.is_empty() {
        return 0.0;
    }
    let shared_count = media_tokens
        .iter()
        .filter(|token| subtitle_tokens.iter().any(|item| item == *token))
        .count();
    let token_score =
        (2.0 * shared_count as f64) / (media_tokens.len() + subtitle_tokens.len()) as f64;
    if shared_count >= 2 && token_score >= 0.55 {
        return token_score * 80.0;
    }
    if shared_count >= 1 && token_score >= 0.75 {
        return token_score * 75.0;
    }
    0.0
}

fn matching_subtitle_entries(
    media_names: &[String],
    media_path: &str,
    entries: Vec<SubtitleMatchEntry>,
) -> Vec<SubtitleMatchEntry> {
    let media_path = media_path.trim();
    let mut scored = entries
        .into_iter()
        .filter(|entry| entry.path.trim() != media_path && is_subtitle_file(&entry.path))
        .filter_map(|entry| {
            let score = media_names
                .iter()
                .map(|media_name| subtitle_match_score(media_name, &entry.name))
                .fold(0.0, f64::max);
            if score <= 0.0 {
                return None;
            }
            Some((score, entry.name.to_lowercase(), entry))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
    });
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

fn resolve_network_subtitle_url(
    connection: &crate::store::network_connection_store::NetworkConnectionRecord,
    protocol_hint: &str,
    path: &str,
) -> Result<String, String> {
    let playback_url =
        crate::network::service::resolve_network_playback_url(connection, Some(protocol_hint), path)?;
    let playback_url = crate::mpv::prepare_network_stream_url(
        protocol_hint,
        &playback_url,
        &connection.username,
        &connection.password,
        connection.tls_certificate_der.as_deref(),
    )?;
    Ok(crate::mpv::rewrite_network_stream_url(protocol_hint, &playback_url)
        .unwrap_or(playback_url))
}

async fn network_subtitle_matches(
    app: &tauri::AppHandle,
    connection_id: &str,
    protocol: crate::network::service::BrowseProtocol,
    current_path: &str,
    parent_path: &str,
    media_title: Option<&str>,
) -> Result<Vec<ExternalSubtitleMatch>, String> {
    let connection =
        crate::store::network_connection_store::find_network_connection(app, connection_id)?;
    let result =
        crate::network::service::browse_connection(app, &connection, parent_path, protocol).await?;
    let current_entry = result
        .entries
        .iter()
        .find(|entry| entry.entry_type == "file" && entry.path == current_path);
    let primary_media_name = if let Some(value) = media_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        value.to_string()
    } else if let Some(entry) = current_entry {
        entry.name.clone()
    } else {
        path_file_name(current_path)
    };
    let fallback_media_name = current_entry
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| path_file_name(current_path));
    let mut media_names = vec![primary_media_name];
    if !media_names.iter().any(|item| item == &fallback_media_name) {
        media_names.push(fallback_media_name.clone());
    }
    let mut entries = result
        .entries
        .iter()
        .filter(|entry| entry.entry_type == "file")
        .map(|entry| SubtitleMatchEntry {
            name: entry.name.clone(),
            path: entry.path.clone(),
        })
        .collect::<Vec<_>>();
    let subtitle_folder_paths = result
        .entries
        .iter()
        .filter(|entry| {
            entry.entry_type == "dir"
                && is_matching_subtitle_folder(&fallback_media_name, &entry.name)
        })
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    for folder_path in subtitle_folder_paths {
        let Ok(folder_result) =
            crate::network::service::browse_connection(app, &connection, &folder_path, protocol)
                .await
        else {
            continue;
        };
        entries.extend(
            folder_result
                .entries
                .into_iter()
                .filter(|entry| entry.entry_type == "file")
                .map(|entry| SubtitleMatchEntry {
                    name: nested_subtitle_match_name(&fallback_media_name, &entry.name),
                    path: entry.path,
                }),
        );
    }
    let matches = matching_subtitle_entries(&media_names, current_path, entries);
    Ok(matches
        .into_iter()
        .filter_map(|entry| {
            let path = match protocol {
                crate::network::service::BrowseProtocol::Webdav => {
                    resolve_network_subtitle_url(&connection, "webdav", &entry.path).ok()?
                }
                crate::network::service::BrowseProtocol::Smb => {
                    resolve_network_subtitle_url(&connection, "smb", &entry.path).ok()?
                }
                crate::network::service::BrowseProtocol::Dlna => entry.path,
            };
            Some(ExternalSubtitleMatch {
                path,
                name: entry.name,
            })
        })
        .collect())
}

#[tauri::command]
pub(crate) async fn find_fuzzy_external_subtitle_matches(
    app: tauri::AppHandle,
    payload: ExternalSubtitleMatchesPayload,
) -> Result<Vec<ExternalSubtitleMatch>, String> {
    match parse_playback_source(&payload.playback_key) {
        PlaybackSource::Local { path } => {
            let Some(local_path) = resolve_local_media_path(&path) else {
                return Ok(Vec::new());
            };
            if !local_path.is_file() {
                return Ok(Vec::new());
            }
            let Some(parent) = local_path.parent() else {
                return Ok(Vec::new());
            };
            let primary_media_name = payload
                .media_title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| path_file_name(&path));
            let fallback_media_name = local_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| path_file_name(&path));
            let mut media_names = vec![primary_media_name];
            if !media_names.iter().any(|item| item == &fallback_media_name) {
                media_names.push(fallback_media_name.clone());
            }
            let parent_entries = std::fs::read_dir(parent)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            let mut entries = parent_entries
                .iter()
                .filter(|entry_path| entry_path.is_file())
                .map(|entry_path| SubtitleMatchEntry {
                    name: entry_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    path: entry_path.to_string_lossy().into_owned(),
                })
                .collect::<Vec<_>>();
            let subtitle_dirs = parent_entries
                .iter()
                .filter(|entry_path| entry_path.is_dir())
                .filter(|entry_path| {
                    let folder_name = entry_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default();
                    is_matching_subtitle_folder(&fallback_media_name, folder_name)
                })
                .cloned()
                .collect::<Vec<_>>();
            for subtitle_dir in subtitle_dirs {
                let Ok(child_entries) = std::fs::read_dir(subtitle_dir) else {
                    continue;
                };
                entries.extend(
                    child_entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|entry_path| entry_path.is_file())
                        .map(|entry_path| SubtitleMatchEntry {
                            name: nested_subtitle_match_name(
                                &fallback_media_name,
                                entry_path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or_default(),
                            ),
                            path: entry_path.to_string_lossy().into_owned(),
                        }),
                );
            }
            Ok(matching_subtitle_entries(&media_names, &path, entries)
                .into_iter()
                .map(|entry| ExternalSubtitleMatch {
                    path: entry.path,
                    name: entry.name,
                })
                .collect())
        }
        PlaybackSource::Webdav {
            connection_id,
            file_path,
        } => {
            let Some(parent_path) = path_parent(&file_path) else {
                return Ok(Vec::new());
            };
            network_subtitle_matches(
                &app,
                &connection_id,
                crate::network::service::BrowseProtocol::Webdav,
                &file_path,
                &parent_path,
                payload.media_title.as_deref(),
            )
            .await
        }
        PlaybackSource::Dlna {
            connection_id,
            resource_url,
            parent_path,
        } => {
            let parent_path = parent_path.unwrap_or_else(|| "0".to_string());
            network_subtitle_matches(
                &app,
                &connection_id,
                crate::network::service::BrowseProtocol::Dlna,
                &resource_url,
                &parent_path,
                payload.media_title.as_deref(),
            )
            .await
        }
        PlaybackSource::Smb {
            connection_id: Some(connection_id),
            file_path: Some(file_path),
            ..
        } => {
            let Some(parent_path) = path_parent(&file_path) else {
                return Ok(Vec::new());
            };
            network_subtitle_matches(
                &app,
                &connection_id,
                crate::network::service::BrowseProtocol::Smb,
                &file_path,
                &parent_path,
                payload.media_title.as_deref(),
            )
            .await
        }
        PlaybackSource::Smb { .. } | PlaybackSource::DirectSmbUrl => Ok(Vec::new()),
    }
}
