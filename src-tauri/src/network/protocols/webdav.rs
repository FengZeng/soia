use crate::store::network_connection_store::NetworkConnectionRecord;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::Method;
use std::cmp::Ordering;
use std::time::Duration;
use url::Url;

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
    <d:getcontentlength/>
    <d:getlastmodified/>
    <d:creationdate/>
  </d:prop>
</d:propfind>"#;

// URL path segment encoding set for playback URLs.
// Keep this strict enough for WebDAV servers that reject reserved chars like '[' and ']'.
const PLAYBACK_PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'[')
    .add(b']');

#[derive(Clone)]
pub struct WebdavBrowseEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified_at: Option<String>,
    pub created_at: Option<String>,
}

pub struct WebdavBrowseResult {
    pub path: String,
    pub entries: Vec<WebdavBrowseEntry>,
}

pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }

    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    };

    with_leading.trim_end_matches('/').to_string()
}

fn decode_segment(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().to_string()
}

fn decode_path_segments(url: &Url) -> Vec<String> {
    url.path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(decode_segment)
                .collect()
        })
        .unwrap_or_default()
}

fn encode_segment_for_playback(segment: &str) -> String {
    let decoded = percent_decode_str(segment).decode_utf8_lossy();
    utf8_percent_encode(decoded.as_ref(), PLAYBACK_PATH_SEGMENT_ENCODE_SET).to_string()
}

fn normalize_playback_url_path(url: &mut Url) {
    let encoded_segments: Vec<String> = url
        .path_segments()
        .map(|segments| segments.map(encode_segment_for_playback).collect())
        .unwrap_or_default();

    let encoded_path = if encoded_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", encoded_segments.join("/"))
    };
    url.set_path(&encoded_path);
}

fn parse_base_url(connection: &NetworkConnectionRecord) -> Result<Url, String> {
    let normalized_base_url = super::normalize_http_base_url(&connection.base_url);
    let mut url =
        Url::parse(&normalized_base_url).map_err(|e| format!("Invalid WebDAV URL: {}", e))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("WebDAV URL must start with http:// or https://".into());
    }
    if url.cannot_be_a_base() {
        return Err("WebDAV URL must be hierarchical".into());
    }
    if url.path().is_empty() {
        url.set_path("/");
    }
    Ok(url)
}

fn build_target_url(base: &Url, root_segments: &[String], path: &str) -> Result<Url, String> {
    let mut url = base.clone();
    let normalized = normalize_path(path);
    let mut target_segments = root_segments.to_vec();
    if normalized != "/" {
        target_segments.extend(
            normalized
                .trim_start_matches('/')
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(|segment| segment.to_string()),
        );
    }

    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "WebDAV URL path cannot be modified".to_string())?;
        segments.clear();
        for segment in target_segments {
            segments.push(&segment);
        }
    }

    if url.path().is_empty() {
        url.set_path("/");
    }

    Ok(url)
}

fn apply_auth(
    request: reqwest::RequestBuilder,
    connection: &NetworkConnectionRecord,
) -> reqwest::RequestBuilder {
    let username = connection.username.trim();
    if username.is_empty() {
        return request;
    }
    let password = if connection.password.is_empty() {
        None
    } else {
        Some(connection.password.clone())
    };
    request.basic_auth(username.to_string(), password)
}

fn node_text(node: roxmltree::Node<'_, '_>, tag_name: &str) -> Option<String> {
    node.descendants()
        .find(|item| item.is_element() && item.tag_name().name().eq_ignore_ascii_case(tag_name))
        .and_then(|item| item.text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn href_to_absolute_url(request_url: &Url, href: &str) -> Option<Url> {
    Url::parse(href)
        .ok()
        .or_else(|| request_url.join(href).ok())
}

fn to_connection_path(root_segments: &[String], url: &Url) -> Option<String> {
    let segments = decode_path_segments(url);
    if segments.len() < root_segments.len() {
        return None;
    }
    if !segments
        .iter()
        .zip(root_segments.iter())
        .all(|(a, b)| a == b)
    {
        return None;
    }

    let relative = &segments[root_segments.len()..];
    if relative.is_empty() {
        return Some("/".to_string());
    }
    Some(format!("/{}", relative.join("/")))
}

pub async fn list_directory(
    app: &tauri::AppHandle,
    connection: &NetworkConnectionRecord,
    path: &str,
) -> Result<WebdavBrowseResult, String> {
    let base_url = parse_base_url(connection)?;
    let root_segments = decode_path_segments(&base_url);
    let normalized_path = normalize_path(path);
    let target_url = build_target_url(&base_url, &root_segments, &normalized_path)?;

    let client = crate::network::proxy::configure_client_builder(
        app,
        reqwest::Client::builder().timeout(Duration::from_secs(15)),
    )?
    .build()
        .map_err(|e| e.to_string())?;
    let propfind = Method::from_bytes(b"PROPFIND").map_err(|e| e.to_string())?;
    let request = client
        .request(propfind, target_url.clone())
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(PROPFIND_BODY.to_string());
    let response = apply_auth(request, connection)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("WebDAV browse failed: {}", status));
    }

    let body = response.text().await.map_err(|e| e.to_string())?;
    let document = roxmltree::Document::parse(&body).map_err(|e| e.to_string())?;

    let mut entries: Vec<WebdavBrowseEntry> = Vec::new();
    for response_node in document
        .descendants()
        .filter(|item| item.is_element() && item.tag_name().name().eq_ignore_ascii_case("response"))
    {
        let Some(href) = node_text(response_node, "href") else {
            continue;
        };
        let Some(href_url) = href_to_absolute_url(&target_url, &href) else {
            continue;
        };
        let Some(connection_path) = to_connection_path(&root_segments, &href_url) else {
            continue;
        };

        if normalize_path(&connection_path) == normalized_path {
            continue;
        }

        let is_dir = response_node.descendants().any(|item| {
            item.is_element() && item.tag_name().name().eq_ignore_ascii_case("collection")
        });
        let name = connection_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }

        let size =
            node_text(response_node, "getcontentlength").and_then(|value| value.parse().ok());
        let modified_at = node_text(response_node, "getlastmodified");
        let created_at = node_text(response_node, "creationdate");

        entries.push(WebdavBrowseEntry {
            name,
            path: connection_path,
            is_dir,
            size,
            modified_at,
            created_at,
        });
    }

    entries.sort_by(|left, right| match (left.is_dir, right.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });

    Ok(WebdavBrowseResult {
        path: normalized_path,
        entries,
    })
}

pub fn build_playback_url(
    connection: &NetworkConnectionRecord,
    file_path: &str,
) -> Result<String, String> {
    let base_url = parse_base_url(connection)?;
    let root_segments = decode_path_segments(&base_url);
    let mut target_url = build_target_url(&base_url, &root_segments, file_path)?;
    normalize_playback_url_path(&mut target_url);
    Ok(target_url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_playback_url, parse_base_url};
    use crate::store::network_connection_store::NetworkConnectionRecord;

    fn connection(base_url: &str) -> NetworkConnectionRecord {
        NetworkConnectionRecord {
            id: "connection".to_string(),
            label: "WebDAV".to_string(),
            protocol: "webdav".to_string(),
            base_url: base_url.to_string(),
            username: String::new(),
            password: String::new(),
            default_path: "/".to_string(),
        }
    }

    #[test]
    fn keeps_custom_ports_when_the_scheme_is_missing() {
        for base_url in ["192.168.31.25:5244/dav", "nas:5244/dav", "nas.local:5244"] {
            let url = parse_base_url(&connection(base_url))
                .unwrap_or_else(|error| panic!("{base_url}: {error}"));
            assert_eq!(url.scheme(), "http");
            assert_eq!(url.port(), Some(5244));
        }
    }

    #[test]
    fn keeps_explicit_schemes_and_ports() {
        let url = parse_base_url(&connection("https://example.com:8443/webdav")).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.port(), Some(8443));
        assert_eq!(url.path(), "/webdav");
    }

    #[test]
    fn rejects_non_http_schemes() {
        let error = parse_base_url(&connection("smb://nas/media")).unwrap_err();
        assert!(error.contains("http://"), "unexpected error: {error}");
    }

    #[test]
    fn playback_url_keeps_the_custom_port() {
        let url = build_playback_url(&connection("192.168.31.25:5244/dav"), "/movies/a b[1].mkv")
            .unwrap();
        assert_eq!(url, "http://192.168.31.25:5244/dav/movies/a%20b%5B1%5D.mkv");
    }
}
