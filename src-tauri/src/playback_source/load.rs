use super::resolve::ResolvedPlaybackSourceResult;
use crate::core::playback_loading::LoadCommandMode;

pub(crate) struct PreparedPlaybackSource {
    pub playback_url: String,
    pub mpv_load_options: Vec<String>,
    pub command_mode: LoadCommandMode,
    pub title: Option<String>,
    pub is_live_playback: bool,
}

pub(crate) fn escape_mpv_load_option_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace(',', "\\,")
}

fn network_title_options(file_path: &str) -> (Option<String>, Vec<String>) {
    let title = crate::playback_source::key::path_file_name(file_path)
        .trim()
        .to_string();
    let title = if title.is_empty() { None } else { Some(title) };
    let options = title
        .as_deref()
        .map(|title| {
            vec![format!(
                "force-media-title={}",
                escape_mpv_load_option_value(title)
            )]
        })
        .unwrap_or_default();
    (title, options)
}

async fn prepare_direct_source(
    app: &tauri::AppHandle,
    url: String,
) -> PreparedPlaybackSource {
    let resolved_media = crate::mpv::try_resolve_with_ytdlp(app, &url).await;
    let playback_url = resolved_media
        .as_ref()
        .map(|resolved| resolved.url.clone())
        .unwrap_or(url);
    let title = resolved_media
        .as_ref()
        .and_then(|resolved| resolved.title.clone());
    let mpv_load_options = title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| {
            vec![format!(
                "force-media-title={}",
                escape_mpv_load_option_value(title)
            )]
        })
        .unwrap_or_default();
    PreparedPlaybackSource {
        playback_url,
        mpv_load_options,
        command_mode: LoadCommandMode::Normal,
        title,
        is_live_playback: resolved_media
            .as_ref()
            .map(|resolved| resolved.is_live_playback)
            .unwrap_or(false),
    }
}

fn prepare_network_source(
    app: &tauri::AppHandle,
    protocol: &str,
    connection_id: &str,
    file_path: &str,
) -> Result<PreparedPlaybackSource, String> {
    let connection = crate::store::network_connection_store::find_network_connection(
        app,
        connection_id,
    )?;
    let mut playback_url = crate::network::service::resolve_network_playback_url(
        &connection,
        Some(protocol),
        file_path,
    )?;
    playback_url = crate::mpv::prepare_network_stream_url(
        protocol,
        &playback_url,
        &connection.username,
        &connection.password,
    )?;
    if let Some(rewritten) = crate::mpv::rewrite_network_stream_url(protocol, &playback_url) {
        playback_url = rewritten;
    }
    let (title, mpv_load_options) = network_title_options(file_path);
    Ok(PreparedPlaybackSource {
        playback_url,
        mpv_load_options,
        command_mode: LoadCommandMode::Direct,
        title,
        is_live_playback: false,
    })
}

pub(crate) async fn prepare(
    app: &tauri::AppHandle,
    source: ResolvedPlaybackSourceResult,
) -> Result<PreparedPlaybackSource, String> {
    match source {
        ResolvedPlaybackSourceResult::Local { file_path, .. } => {
            Ok(prepare_direct_source(app, file_path).await)
        }
        ResolvedPlaybackSourceResult::Webdav {
            connection_id,
            file_path,
            ..
        } => prepare_network_source(app, "webdav", &connection_id, &file_path),
        ResolvedPlaybackSourceResult::Dlna { resource_url, .. }
        | ResolvedPlaybackSourceResult::DirectSmb { resource_url, .. } => {
            Ok(prepare_direct_source(app, resource_url).await)
        }
        ResolvedPlaybackSourceResult::Smb {
            connection_id,
            file_path,
            ..
        } => prepare_network_source(app, "smb", &connection_id, &file_path),
    }
}

#[cfg(test)]
mod tests {
    use super::{escape_mpv_load_option_value, network_title_options};

    #[test]
    fn escapes_mpv_title_option_delimiters() {
        assert_eq!(
            escape_mpv_load_option_value("one,two\\three"),
            "one\\,two\\\\three",
        );
    }

    #[test]
    fn uses_network_file_name_as_media_title() {
        let (title, options) = network_title_options(
            "/shows/4K_0b51087c2ed03b86bb9ffe3fcb9f8fbc.mp4",
        );

        assert_eq!(title.as_deref(), Some("4K_0b51087c2ed03b86bb9ffe3fcb9f8fbc.mp4"));
        assert_eq!(
            options,
            vec!["force-media-title=4K_0b51087c2ed03b86bb9ffe3fcb9f8fbc.mp4"]
        );
    }
}
