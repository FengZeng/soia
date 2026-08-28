use tauri::AppHandle;

pub(crate) struct ResolvedMpvMedia {
    pub(crate) url: String,
    pub(crate) title: Option<String>,
    pub(crate) is_live_playback: bool,
}

/// Adapts generic yt-dlp stream descriptors to mpv input URLs. This is intentionally separate
/// from `crate::ytdlp`: both the loopback gateway registration and mpv's EDL syntax are playback
/// backend concerns, not yt-dlp resolution concerns.
pub(crate) async fn try_resolve(app: &AppHandle, raw_url: &str) -> Option<ResolvedMpvMedia> {
    let resolved = crate::ytdlp::try_resolve(app, raw_url).await?;
    let url = match resolved.streams.as_slice() {
        [stream] => proxied_stream_url(stream),
        streams => build_edl_url(streams),
    };
    Some(ResolvedMpvMedia {
        url,
        title: resolved.title,
        is_live_playback: resolved.is_live_playback,
    })
}

fn proxied_stream_url(stream: &crate::ytdlp::ResolvedStream) -> String {
    crate::media_gateway::create_loopback_media_url_with_headers(
        &stream.url,
        &stream.headers,
        stream.available_at,
    )
    .unwrap_or_else(|| stream.url.clone())
}

fn build_edl_url(streams: &[crate::ytdlp::ResolvedStream]) -> String {
    let mut edl = String::from("edl://");
    for stream in streams {
        let url = proxied_stream_url(stream);
        edl.push_str(&format!(
            "!new_stream;!no_clip;!no_chapters;%{}%{};",
            url.len(),
            url
        ));
    }
    edl.trim_end_matches(';').to_string()
}
