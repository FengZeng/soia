use crate::playback_source::{parse_playback_source, PlaybackSource};
use crate::store::track_preferences::TrackKind;
use crate::AppState;
use tauri::Manager;
use tokio::sync::oneshot;

pub(crate) struct ExternalTrackRequest {
    pub generation: u64,
    pub playback_key: String,
    pub kind: TrackKind,
    pub source: String,
    pub url: String,
    pub title: String,
    pub select: bool,
    pub completion: oneshot::Sender<Result<bool, String>>,
}

impl ExternalTrackRequest {
    pub(crate) fn finish(self, result: Result<bool, String>) {
        let _ = self.completion.send(result);
    }
}

pub(super) fn is_temporary_source(source: &str) -> bool {
    if crate::media_gateway::is_loopback_media_url(source) { return true; }
    let Ok(url) = url::Url::parse(source) else { return false };
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        && url.path().strip_prefix("/stream/").is_some_and(|token| uuid::Uuid::parse_str(token).is_ok())
}

async fn resolve_source(app: &tauri::AppHandle, source: &str) -> Result<String, String> {
    match parse_playback_source(source) {
        PlaybackSource::Webdav { .. } | PlaybackSource::Smb { .. } | PlaybackSource::DirectSmbUrl => {
            let resolved = crate::playback_source::resolve::resolve(app, source).await?;
            Ok(crate::playback_source::load::prepare(app, resolved).await?.playback_url)
        }
        PlaybackSource::Dlna { resource_url, .. } => {
            Ok(crate::mpv::rewrite_network_stream_url("dlna", &resource_url).unwrap_or(resource_url))
        }
        PlaybackSource::Local { path } => {
            if is_temporary_source(&path) {
                return Err("temporary media gateway URLs are not external track sources".into());
            }
            Ok(crate::mpv::rewrite_network_stream_url("", &path).unwrap_or(path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_temporary_source;

    #[test]
    fn rejects_proxy_sources_from_previous_processes_without_rejecting_local_paths() {
        assert!(is_temporary_source("http://127.0.0.1:31000/stream/019e0000-0000-7000-8000-000000000001"));
        assert!(is_temporary_source("http://[::1]:42000/stream/019e0000-0000-7000-8000-000000000002"));
        assert!(!is_temporary_source("/media/subtitle.ass"));
        assert!(!is_temporary_source("soia-webdav://1/episode.zh.ass"));
        assert!(!is_temporary_source("http://example.test/stream/019e0000-0000-7000-8000-000000000001"));
    }
}

pub(crate) async fn load(
    app: &tauri::AppHandle,
    state: &AppState,
    generation: u64,
    playback_key: &str,
    kind: TrackKind,
    source: &str,
    select: bool,
    title: &str,
) -> Result<bool, String> {
    if state.playback_load_coordinator.current() != generation
        || !state.track_selection.is_current(generation, playback_key)
    {
        return Ok(false);
    }
    // Connection/certificate resolution can perform I/O; keep it off the event
    // thread and outside playback and track-state locks.
    let url = resolve_source(app, source).await?;
    if state.playback_load_coordinator.current() != generation
        || !state.track_selection.register_external_source(generation, kind, source, &url)
    {
        return Ok(false);
    }
    let (completion, receiver) = oneshot::channel();
    state.track_selection.enqueue_external_track(ExternalTrackRequest {
        generation,
        playback_key: playback_key.to_string(),
        kind,
        source: source.to_string(),
        url,
        title: title.to_string(),
        select,
        completion,
    });
    receiver.await.map_err(|_| "external track loader stopped".to_string())?
}

pub(crate) fn restore_saved(
    app: &tauri::AppHandle,
    generation: u64,
    playback_key: &str,
    sources: Vec<(TrackKind, String)>,
) {
    for (kind, source) in sources {
        let app = app.clone();
        let playback_key = playback_key.to_string();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let title = crate::playback_source::path_file_name(&source);
            if let Err(error) = load(&app, &state, generation, &playback_key, kind, &source, false, &title).await {
                log::warn!("failed to restore external {} track: {error}", kind.mpv_type());
            }
        });
    }
}
