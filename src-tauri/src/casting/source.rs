use crate::playback_source::resolve::ResolvedPlaybackSourceResult;
use super::CastMediaDescriptor;
use std::net::Ipv4Addr;
use std::path::PathBuf;
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
            Self::LocalFile { path } => (
                crate::media_gateway::create_cast_local_file_media_url(
                    app,
                    cast_session_id,
                    receiver_ip,
                    &path,
                )?,
                Some(crate::media_gateway::infer_media_mime(&path).to_string()),
            ),
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
                    None,
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
    from_resolved(app, source)
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
    if let Ok(url) = Url::parse(value.trim()) {
        return match url.scheme() {
            "http" | "https" => Ok(CastMediaSource::Http {
                url: url.to_string(),
                basic_auth: None,
            }),
            "smb" => Ok(CastMediaSource::Smb {
                url: url.to_string(),
                basic_auth: None,
            }),
            "file" => Ok(CastMediaSource::LocalFile {
                path: url.to_file_path().map_err(|_| "invalid local file URL".to_string())?,
            }),
            scheme => Err(format!("unsupported direct cast source scheme: {scheme}")),
        };
    }
    let path = crate::playback_source::resolve_local_media_path(value)
        .ok_or_else(|| "invalid local cast media path".to_string())?;
    Ok(CastMediaSource::LocalFile { path })
}

#[cfg(test)]
mod tests {
    use super::{classify_direct_source, CastMediaSource};

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
    }
}
