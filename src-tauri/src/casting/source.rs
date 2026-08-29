use log::info;

use crate::media_gateway::ProgressiveRemuxFormat;
use crate::ytdlp::ResolvedCastStreams;
use crate::playback_source::resolve::ResolvedPlaybackSourceResult;
use super::CastMediaDescriptor;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use url::Url;

/// A Core-only source description created before the media gateway allocates a LAN lease. It
/// keeps credentials out of frontend DTOs and prevents protocol adapters from seeing source URLs.
pub(crate) enum CastMediaSource {
    LocalFile {
        path: PathBuf,
    },
    Http {
        url: String,
        basic_auth: Option<(String, String)>,
    },
    Smb {
        url: String,
        basic_auth: Option<(String, String)>,
    },
    /// The experimental HLS/CMAF transport, retained for receivers which explicitly support it.
    HlsCmafStream {
        video_url: String,
        audio_url: String,
        video_headers: Vec<(String, String)>,
        audio_headers: Vec<(String, String)>,
        video_available_at: Option<i64>,
        audio_available_at: Option<i64>,
    },
    /// A direct, single-URL remux transport for DASH video/audio streams.
    ProgressiveRemuxStream {
        output_format: ProgressiveRemuxFormat,
        video_url: String,
        audio_url: String,
        video_headers: Vec<(String, String)>,
        audio_headers: Vec<(String, String)>,
        video_available_at: Option<i64>,
        audio_available_at: Option<i64>,
    },
}

impl CastMediaSource {
    /// Allocates the session-scoped gateway URL only after the source has been fully resolved by
    /// Core. Neither the lease URL nor its credentials are exposed through frontend DTOs.
    pub(crate) fn create_descriptor(
        self,
        app: tauri::AppHandle,
        cast_session_id: &str,
        receiver_ip: Ipv4Addr,
        title: Option<String>,
        duration: Option<f64>,
        position: f64,
    ) -> Result<CastMediaDescriptor, String> {
        let (url, mime_type) = match self {
            Self::LocalFile { path } => {
                if is_local_hls_path(&path) {
                    return Err(local_hls_not_supported_error());
                }
                (
                    crate::media_gateway::create_cast_local_file_media_url(
                        app,
                        cast_session_id,
                        receiver_ip,
                        &path,
                    )?,
                    Some(crate::media_gateway::infer_media_mime(&path).to_string()),
                )
            }
            Self::Http { url, basic_auth } => {
                if let Some((username, password)) = basic_auth {
                    crate::media_gateway::register_basic_auth(&url, &username, &password);
                }
                (
                    crate::media_gateway::create_cast_http_media_url(
                        app,
                        cast_session_id,
                        receiver_ip,
                        &url,
                    )?,
                    infer_http_mime(&url),
                )
            }
            Self::Smb { url, basic_auth } => {
                if let Some((username, password)) = basic_auth {
                    crate::media_gateway::register_basic_auth(&url, &username, &password);
                }
                (
                    crate::media_gateway::create_cast_smb_media_url(
                        app,
                        cast_session_id,
                        receiver_ip,
                        &url,
                    )?,
                    None,
                )
            }
            Self::HlsCmafStream {
                video_url,
                audio_url,
                video_headers,
                audio_headers,
                video_available_at,
                audio_available_at,
            } => {
                let url = crate::media_gateway::create_cast_hls_cmaf_media_url(
                    app,
                    cast_session_id,
                    receiver_ip,
                    &video_url,
                    &audio_url,
                    &video_headers,
                    &audio_headers,
                    video_available_at,
                    audio_available_at,
                )?;
                (url, Some("application/vnd.apple.mpegurl".to_string()))
            }
            Self::ProgressiveRemuxStream {
                output_format,
                video_url,
                audio_url,
                video_headers,
                audio_headers,
                video_available_at,
                audio_available_at,
            } => {
                let url = crate::media_gateway::create_cast_progressive_remux_media_url(
                    app,
                    cast_session_id,
                    receiver_ip,
                    output_format,
                    &video_url,
                    &audio_url,
                    &video_headers,
                    &audio_headers,
                    video_available_at,
                    audio_available_at,
                )?;
                (
                    url,
                    Some(output_format.mime_type().to_string()),
                )
            }
        };
        Ok(CastMediaDescriptor {
            url,
            title,
            mime_type,
            duration,
            position,
        })
    }
}

pub(crate) async fn resolve(
    app: &tauri::AppHandle,
    playback_key: &str,
) -> Result<CastMediaSource, String> {
    let source = crate::playback_source::resolve::resolve(app, playback_key).await?;
    let media_source = from_resolved(app, source)?;
    let CastMediaSource::Http {
        url,
        basic_auth: None,
        ..
    } = &media_source else {
        return Ok(media_source);
    };
    if !needs_ytdlp_resolution(url) {
        return Ok(media_source);
    }
    let streams = match crate::ytdlp::resolve_for_cast(app, url).await {
        Ok(Some(streams)) => streams,
        Ok(None) => return Err("yt-dlp did not return a castable stream".to_string()),
        Err(error) => return Err(error),
    };
    // Past this point the original URL is a webpage, not media. Falling back to it would leave the
    // receiver spinning on HTML, so a failure here is reported instead of swallowed.
    Ok(cast_source_from_streams(streams))
}

fn needs_ytdlp_resolution(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let path = parsed.path().to_ascii_lowercase();
    let direct_extensions = [
        "m3u8", "mp4", "m4v", "mov", "mkv", "webm", "flv", "avi", "ts", "mp3", "m4a", "aac",
        "flac", "wav", "ogg", "opus",
    ];
    !direct_extensions
        .iter()
        .any(|ext| path.ends_with(&format!(".{ext}")))
}

fn cast_source_from_streams(resolved: ResolvedCastStreams) -> CastMediaSource {
    match resolved {
        ResolvedCastStreams::Single { url, headers } => {
            if !headers.is_empty() {
                crate::media_gateway::register_headers(&url, &headers);
            }
            info!("casting: serving a single muxed yt-dlp stream");
            CastMediaSource::Http {
                url,
                basic_auth: None,
            }
        }
        ResolvedCastStreams::VideoAudio {
            video_url,
            audio_url,
            video_headers,
            audio_headers,
            video_available_at,
            audio_available_at,
        } => {
            let output_format = ProgressiveRemuxFormat::selected_for_cast();
            info!(
                "casting: remuxing separate yt-dlp streams to progressive {}",
                match output_format {
                    ProgressiveRemuxFormat::MpegTs => "MPEG-TS",
                    ProgressiveRemuxFormat::FragmentedMp4 => "fragmented MP4",
                }
            );
            CastMediaSource::ProgressiveRemuxStream {
                output_format,
                video_url,
                audio_url,
                video_headers,
                audio_headers,
                video_available_at,
                audio_available_at,
            }
        }
    }
}

fn from_resolved(
    app: &tauri::AppHandle,
    source: ResolvedPlaybackSourceResult,
) -> Result<CastMediaSource, String> {
    match source {
        ResolvedPlaybackSourceResult::Local { file_path, .. } => classify_direct_source(&file_path),
        ResolvedPlaybackSourceResult::Dlna { resource_url, .. } => classify_direct_source(&resource_url),
        ResolvedPlaybackSourceResult::DirectSmb { resource_url, .. } => classify_direct_source(&resource_url),
        ResolvedPlaybackSourceResult::Webdav {
            connection_id,
            file_path,
            ..
        } => resolve_network_source(app, "webdav", &connection_id, &file_path),
        ResolvedPlaybackSourceResult::Smb {
            connection_id,
            file_path,
            ..
        } => resolve_network_source(app, "smb", &connection_id, &file_path),
    }
}

fn resolve_network_source(
    app: &tauri::AppHandle,
    protocol: &str,
    connection_id: &str,
    file_path: &str,
) -> Result<CastMediaSource, String> {
    let connection = crate::store::network_connection_store::find_network_connection(app, connection_id)?;
    let url = crate::network::service::resolve_network_playback_url(
        &connection,
        Some(protocol),
        file_path,
    )?;
    let basic_auth = (!connection.username.trim().is_empty())
        .then(|| (connection.username.trim().to_string(), connection.password));
    match protocol {
        "webdav" => Ok(CastMediaSource::Http { url, basic_auth }),
        "smb" => Ok(CastMediaSource::Smb { url, basic_auth }),
        _ => Err(format!("unsupported cast network protocol: {protocol}")),
    }
}

fn classify_direct_source(value: &str) -> Result<CastMediaSource, String> {
    let trimmed = value.trim();
    // `url::Url::parse` interprets a Windows drive letter (for example,
    // `E:\\Movies\\film.mkv`) as a URI scheme (`e`). Check for drive paths
    // before attempting URL parsing so local files are not rejected as an
    // unsupported direct-cast scheme.
    if is_windows_drive_path(trimmed) {
        let path = crate::playback_source::resolve_local_media_path(trimmed)
            .ok_or_else(|| "invalid local cast media path".to_string())?;
        return local_file_source(path);
    }

    if let Ok(url) = Url::parse(trimmed) {
        return match url.scheme() {
            "http" | "https" => Ok(CastMediaSource::Http {
                url: url.to_string(),
                basic_auth: None,
            }),
            "smb" => Ok(CastMediaSource::Smb {
                url: url.to_string(),
                basic_auth: None,
            }),
            "file" => local_file_source(
                url.to_file_path().map_err(|_| "invalid local file URL".to_string())?,
            ),
            scheme => Err(format!("unsupported direct cast source scheme: {scheme}")),
        };
    }
    let path = crate::playback_source::resolve_local_media_path(trimmed)
        .ok_or_else(|| "invalid local cast media path".to_string())?;
    local_file_source(path)
}

fn is_windows_drive_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn local_file_source(path: PathBuf) -> Result<CastMediaSource, String> {
    if is_local_hls_path(&path) {
        return Err(local_hls_not_supported_error());
    }
    Ok(CastMediaSource::LocalFile { path })
}

fn is_local_hls_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if extension.eq_ignore_ascii_case("m3u8") || extension.eq_ignore_ascii_case("m3u")
    )
}

fn local_hls_not_supported_error() -> String {
    "local HLS casting is unavailable until restricted child-resource authorization is implemented"
        .to_string()
}

fn infer_http_mime(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let mime_type = crate::media_gateway::infer_media_mime(Path::new(parsed.path()));
    (mime_type != "application/octet-stream").then(|| mime_type.to_string())
}

#[cfg(test)]
mod tests {
    use super::{classify_direct_source, infer_http_mime, needs_ytdlp_resolution, CastMediaSource};

    #[test]
    fn classifies_local_http_and_smb_sources_without_leaking_them_to_dtos() {
        assert!(matches!(
            classify_direct_source("/Movies/example.mkv").unwrap(),
            CastMediaSource::LocalFile { .. }
        ));
        assert!(matches!(
            classify_direct_source("https://media.example.test/video.mp4").unwrap(),
            CastMediaSource::Http { basic_auth: None, .. }
        ));
        assert!(matches!(
            classify_direct_source("smb://nas.example.test/media/video.mkv").unwrap(),
            CastMediaSource::Smb { basic_auth: None, .. }
        ));
        let error = match classify_direct_source("/Movies/playlist.m3u8") {
            Err(error) => error,
            Ok(_) => panic!("local HLS must not be cast before child-resource authorization exists"),
        };
        assert!(error.contains("restricted child-resource authorization"));
    }

    #[test]
    fn detects_urls_needing_ytdlp_resolution() {
        assert!(needs_ytdlp_resolution("https://www.bilibili.com/video/BV1ks8n6rEWe"));
        assert!(needs_ytdlp_resolution("https://www.youtube.com/watch?v=abc123"));
        assert!(!needs_ytdlp_resolution("https://cdn.example.com/video.mp4"));
        assert!(!needs_ytdlp_resolution("https://cdn.example.com/stream.m3u8"));
        assert!(!needs_ytdlp_resolution("/local/path/video.mkv"));
    }

    #[test]
    fn infers_didl_mime_for_known_http_media_extensions() {
        assert_eq!(
            infer_http_mime("https://cdn.example.test/movie.MKV?token=redacted"),
            Some("video/x-matroska".to_string()),
        );
        assert_eq!(
            infer_http_mime("https://cdn.example.test/live/index.m3u8"),
            Some("application/vnd.apple.mpegurl".to_string()),
        );
        assert_eq!(infer_http_mime("https://cdn.example.test/media"), None);
    }
}
