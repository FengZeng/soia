use log::{info, warn};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use url::Url;

mod settings;

pub(crate) use settings::{store_runtime_settings, YtdlpFormatSettings, YtdlpSettings};

const YTDLP_TIMEOUT: Duration = Duration::from_secs(60);
const YTDLP_RESOLUTION_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DIRECT_STREAM_EXTENSIONS: &[&str] = &[
    "m3u8", "mp4", "m4v", "mov", "mkv", "webm", "flv", "avi", "ts", "mp3", "m4a", "aac", "flac",
    "wav", "ogg", "opus",
];

#[derive(Clone)]
struct Candidate {
    url: String,
    headers: Vec<(String, String)>,
    available_at: Option<i64>,
    format_id: Option<String>,
    protocol: Option<String>,
    resolution: Option<String>,
    score: i64,
}

pub(crate) struct ResolvedStream {
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) available_at: Option<i64>,
}

pub(crate) struct ResolvedMedia {
    pub(crate) streams: Vec<ResolvedStream>,
    pub(crate) title: Option<String>,
    pub(crate) is_live_playback: bool,
}

pub(crate) struct ResolvedPlaylistEntry {
    pub(crate) url: String,
    pub(crate) title: Option<String>,
}

pub(crate) struct ResolvedPlaylist {
    pub(crate) title: Option<String>,
    pub(crate) entries: Vec<ResolvedPlaylistEntry>,
}

pub(crate) async fn resolve_playlist(
    app: &AppHandle,
    raw_url: &str,
) -> Result<ResolvedPlaylist, String> {
    let settings = settings::resolve(app);
    let Some(ytdl_path) = settings.binary.path else {
        return Err("yt-dlp is not configured".to_string());
    };

    let proxy_url = crate::network::proxy::current_proxy_key(app)?;
    let cookies_from_browser = settings.cookies.browser;
    let raw_url = raw_url.to_string();
    let cookies_clone = cookies_from_browser.clone();
    let proxy_clone = proxy_url.clone();
    let url_clone = raw_url.clone();
    let ytdl_clone = ytdl_path.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        run_ytdlp_playlist_command(&ytdl_path, proxy_url.as_deref(), cookies_from_browser.as_deref(), &raw_url)
    })
    .await
    .map_err(|error| format!("yt-dlp worker failed: {error}"))??;

    let output = if !output.status.success()
        && cookies_clone.is_some()
        && is_cookie_permission_error(&output.stderr)
    {
        warn!(
            "yt-dlp: cookies-from-browser failed due to permission error, retrying without cookies"
        );
        tauri::async_runtime::spawn_blocking(move || {
            run_ytdlp_playlist_command(&ytdl_clone, proxy_clone.as_deref(), None, &url_clone)
        })
        .await
        .map_err(|error| format!("yt-dlp worker failed: {error}"))??
    } else {
        output
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "yt-dlp exited with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("yt-dlp returned invalid JSON: {error}"))?;

    let entries = extract_playlist_entries(&value);
    if entries.is_empty() {
        return Err("yt-dlp did not return any playlist entries".to_string());
    }

    let title = extract_media_title(&value);
    info!(
        "yt-dlp: resolved {} playlist entries title={:?}",
        entries.len(),
        title
    );
    Ok(ResolvedPlaylist { title, entries })
}

fn run_ytdlp_playlist_command(
    ytdl_path: &str,
    proxy_url: Option<&str>,
    cookies_from_browser: Option<&str>,
    raw_url: &str,
) -> Result<std::process::Output, String> {
    let mut command = Command::new(ytdl_path);
    hide_windows_console(&mut command);
    let mut log_args = vec![
        "--dump-single-json".to_string(),
        "--flat-playlist".to_string(),
        redact_url(raw_url),
    ];
    command
        .arg("--dump-single-json")
        .arg("--flat-playlist")
        .arg(raw_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(proxy_url) = proxy_url {
        command.arg("--proxy").arg(proxy_url);
        log_args.push("--proxy".to_string());
        log_args.push(redact_url(proxy_url));
    }

    if let Some(browser) = cookies_from_browser {
        command.arg("--cookies-from-browser").arg(browser);
        log_args.push("--cookies-from-browser".to_string());
        log_args.push(browser.to_string());
    }

    info!(
        "yt-dlp: run {}",
        format_command_for_log(ytdl_path, &log_args)
    );

    let mut child = command
        .spawn()
        .map_err(|error| format!("yt-dlp failed to start: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "yt-dlp stdout pipe is unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "yt-dlp stderr pipe is unavailable".to_string())?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started_at = Instant::now();
    let deadline = Instant::now() + YTDLP_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("yt-dlp wait failed: {error}"))?
        {
            let elapsed = started_at.elapsed();
            info!("yt-dlp: playlist finished in {:.3}s", elapsed.as_secs_f64());
            let stdout = stdout_reader
                .join()
                .map_err(|_| "yt-dlp stdout reader panicked".to_string())?
                .map_err(|error| format!("yt-dlp stdout read failed: {error}"))?;
            let stderr = stderr_reader
                .join()
                .map_err(|_| "yt-dlp stderr reader panicked".to_string())?
                .map_err(|error| format!("yt-dlp stderr read failed: {error}"))?;
            let output = std::process::Output {
                status,
                stdout,
                stderr,
            };
            info!(
                "yt-dlp: playlist exit status={} stdout={}B stderr={}B",
                output.status,
                output.stdout.len(),
                output.stderr.len()
            );
            return Ok(output);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            warn!(
                "yt-dlp: playlist timed out after {:.3}s",
                started_at.elapsed().as_secs_f64()
            );
            return Err(format!(
                "yt-dlp timed out after {}s",
                YTDLP_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn extract_playlist_entries(value: &Value) -> Vec<ResolvedPlaylistEntry> {
    value
        .get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let url = entry
                .get("url")
                .and_then(Value::as_str)
                .or_else(|| entry.get("webpage_url").and_then(Value::as_str))
                .filter(|url| !url.is_empty())?;
            let title = entry
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            Some(ResolvedPlaylistEntry {
                url: url.to_string(),
                title,
            })
        })
        .collect()
}

pub(crate) async fn resolve(app: &AppHandle, raw_url: &str) -> Result<Option<ResolvedMedia>, String> {
    if !is_http_url(raw_url) {
        return Ok(None);
    }
    if is_likely_direct_stream_url(raw_url) {
        return Ok(None);
    }

    let settings = settings::resolve(app);
    let Some(ytdl_path) = settings.binary.path else {
        return Ok(None);
    };

    let proxy_url = crate::network::proxy::current_proxy_key(app)?;
    let cookies_from_browser = settings.cookies.browser;
    let format_selector = settings.format.selector();
    let raw_url = raw_url.to_string();
    let cookies_clone = cookies_from_browser.clone();
    let proxy_clone = proxy_url.clone();
    let format_clone = format_selector.clone();
    let url_clone = raw_url.clone();
    let command_url = raw_url.clone();
    let ytdl_clone = ytdl_path.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        run_ytdlp_command(
            &ytdl_path,
            proxy_url.as_deref(),
            cookies_from_browser.as_deref(),
            &format_selector,
            &command_url,
        )
    })
    .await
    .map_err(|error| format!("yt-dlp worker failed: {error}"))??;
    let output = if !output.status.success()
        && cookies_clone.is_some()
        && is_cookie_permission_error(&output.stderr)
    {
        warn!(
            "yt-dlp: cookies-from-browser failed due to permission error, retrying without cookies"
        );
        tauri::async_runtime::spawn_blocking(move || {
            run_ytdlp_command(
                &ytdl_clone,
                proxy_clone.as_deref(),
                None,
                &format_clone,
                &url_clone,
            )
        })
        .await
        .map_err(|error| format!("yt-dlp worker failed: {error}"))??
    } else {
        output
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "yt-dlp exited with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("yt-dlp returned invalid JSON: {error}"))?;
    let cast_streams = select_cast_streams(&value, settings.format.max_height);
    if let Some(streams) = cast_streams {
        cache_cast_resolution(&raw_url, streams);
    } else {
        log::debug!("yt-dlp: no cast descriptor was produced during playback resolution");
    }
    let candidates = select_candidates(&value);
    if candidates.is_empty() {
        return Err("yt-dlp did not return a playable URL".to_string());
    }
    for candidate in &candidates {
        log_selected_candidate("selected", candidate);
    }
    let is_live_playback = candidates.iter().any(is_likely_live_candidate);
    let title = extract_media_title(&value);

    info!("yt-dlp: resolved {} playable stream(s)", candidates.len());
    Ok(Some(ResolvedMedia {
        streams: candidates
            .into_iter()
            .map(|candidate| ResolvedStream {
                url: candidate.url,
                headers: candidate.headers,
                available_at: candidate.available_at,
            })
            .collect(),
        title,
        is_live_playback,
    }))
}

/// Streams selected for a cast session. Receivers cannot mux DASH video and audio themselves, so
/// separate streams are reported as such and remuxed by the media gateway before they leave the app.
#[derive(Clone)]
pub(crate) enum ResolvedCastStreams {
    Single {
        url: String,
        headers: Vec<(String, String)>,
    },
    VideoAudio {
        video_url: String,
        audio_url: String,
        video_headers: Vec<(String, String)>,
        audio_headers: Vec<(String, String)>,
        video_available_at: Option<i64>,
        audio_available_at: Option<i64>,
    },
}

struct CachedCastResolution {
    resolved_at: Instant,
    streams: ResolvedCastStreams,
}

static YTDLP_CAST_RESOLUTION_CACHE: OnceLock<Mutex<HashMap<String, CachedCastResolution>>> =
    OnceLock::new();

pub(crate) async fn resolve_for_cast(
    app: &AppHandle,
    raw_url: &str,
) -> Result<Option<ResolvedCastStreams>, String> {
    if !is_http_url(raw_url) {
        return Ok(None);
    }
    if is_likely_direct_stream_url(raw_url) {
        return Ok(None);
    }

    let raw_url = raw_url.to_string();
    if let Some(streams) = cached_cast_resolution(&raw_url) {
        info!("yt-dlp: reusing cast descriptor produced during playback resolution");
        log_cast_stream_selection(&streams);
        return Ok(Some(streams));
    }

    let streams = {
        let settings = settings::resolve(app);
        let Some(ytdl_path) = settings.binary.path else {
            return Err("yt-dlp is not configured for webpage casting".to_string());
        };

        let proxy_url = crate::network::proxy::current_proxy_key(app)?;
        let cookies_from_browser = settings.cookies.browser;
        let format_selector = settings.format.cast_selector();
        let cookies_clone = cookies_from_browser.clone();
        let proxy_clone = proxy_url.clone();
        let format_clone = format_selector.clone();
        let url_clone = raw_url.clone();
        let command_url = raw_url.clone();
        let ytdl_clone = ytdl_path.clone();
        let output = tauri::async_runtime::spawn_blocking(move || {
            run_ytdlp_command(
                &ytdl_path,
                proxy_url.as_deref(),
                cookies_from_browser.as_deref(),
                &format_selector,
                &command_url,
            )
        })
        .await
        .map_err(|error| format!("yt-dlp worker failed: {error}"))??;
        let output = if !output.status.success()
            && cookies_clone.is_some()
            && is_cookie_permission_error(&output.stderr)
        {
            warn!(
                "yt-dlp: cookies-from-browser failed due to permission error, retrying without cookies"
            );
            tauri::async_runtime::spawn_blocking(move || {
                run_ytdlp_command(
                    &ytdl_clone,
                    proxy_clone.as_deref(),
                    None,
                    &format_clone,
                    &url_clone,
                )
            })
            .await
            .map_err(|error| format!("yt-dlp worker failed: {error}"))??
        } else {
            output
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "yt-dlp exited with status {}: {}",
                output.status,
                stderr.trim()
            ));
        }

        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("yt-dlp returned invalid JSON: {error}"))?;
        select_cast_streams(&value, settings.format.max_height)
            .ok_or_else(|| "yt-dlp did not return a castable URL".to_string())?
    };
    cache_cast_resolution(&raw_url, streams.clone());
    log_cast_stream_selection(&streams);
    Ok(Some(streams))
}

fn log_cast_stream_selection(streams: &ResolvedCastStreams) {
    match streams {
        ResolvedCastStreams::Single { .. } => {
            info!("yt-dlp: cast selected a combined stream");
        }
        ResolvedCastStreams::VideoAudio { .. } => {
            info!("yt-dlp: cast selected separate video and audio streams for remuxing");
        }
    }
}

fn select_cast_streams(value: &Value, max_height: u32) -> Option<ResolvedCastStreams> {
    let top_headers = parse_headers(value.get("http_headers"));
    let requested: Vec<&Value> = value
        .get("requested_formats")
        .and_then(Value::as_array)
        .map(|formats| formats.iter().collect())
        .unwrap_or_default();
    let video_only = requested
        .iter()
        .copied()
        .find(|format| format_has_video(format) && !format_has_audio(format));
    let requested_height = video_only
        .and_then(|format| format.get("height"))
        .and_then(Value::as_u64);

    if let Some(candidate) = select_best_cast_combined_candidate(
        value,
        &top_headers,
        max_height,
        requested_height,
    ) {
        info!("yt-dlp: cast prefers a compatible combined stream over remuxing");
        log_selected_candidate("cast combined stream", &candidate);
        return Some(ResolvedCastStreams::Single {
            url: candidate.url,
            headers: candidate.headers,
        });
    }

    let requested_audio = requested
        .iter()
        .copied()
        .find(|format| format_has_audio(format) && !format_has_video(format));
    let audio_only = requested_audio.and_then(|audio| {
        select_aac_audio_format(value)
            .or(Some(audio))
    });
    if let (Some(video), Some(audio)) = (video_only, audio_only) {
        let video_url = video.get("url").and_then(Value::as_str).filter(|url| is_http_url(url));
        let audio_url = audio.get("url").and_then(Value::as_str).filter(|url| is_http_url(url));
        if let (Some(video_url), Some(audio_url)) = (video_url, audio_url) {
            log_selected_format("cast video stream", video);
            log_selected_format("cast audio stream", audio);
            return Some(ResolvedCastStreams::VideoAudio {
                video_url: video_url.to_string(),
                audio_url: audio_url.to_string(),
                video_headers: merge_headers(
                    &top_headers,
                    &parse_headers(video.get("http_headers")),
                ),
                audio_headers: merge_headers(
                    &top_headers,
                    &parse_headers(audio.get("http_headers")),
                ),
                video_available_at: video.get("available_at").and_then(Value::as_i64),
                audio_available_at: audio.get("available_at").and_then(Value::as_i64),
            });
        }
    }

    None
}

fn select_best_cast_combined_candidate(
    value: &Value,
    top_headers: &[(String, String)],
    max_height: u32,
    requested_height: Option<u64>,
) -> Option<Candidate> {
    value
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|format| format_has_video(format) && format_has_audio(format))
        .filter(|format| {
            format
                .get("height")
                .and_then(Value::as_u64)
                .is_none_or(|height| height <= u64::from(max_height))
        })
        .filter(|format| {
            requested_height.is_none_or(|height| {
                format.get("height").and_then(Value::as_u64) == Some(height)
            })
        })
        .filter_map(|format| format_candidate(format, top_headers))
        .max_by_key(|candidate| candidate.score)
}

fn select_aac_audio_format(value: &Value) -> Option<&Value> {
    value
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|format| format_has_audio(format) && !format_has_video(format))
        .filter(|format| is_aac_audio_format(format))
        .filter(|format| {
            format
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(is_http_url)
        })
        .max_by_key(|format| audio_format_score(format))
}

fn is_aac_audio_format(format: &Value) -> bool {
    let acodec = format
        .get("acodec")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ext = format
        .get("ext")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let container = format
        .get("container")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    acodec.starts_with("mp4a") || ext == "m4a" || container.starts_with("m4a")
}

fn audio_format_score(format: &Value) -> i64 {
    format
        .get("abr")
        .and_then(Value::as_f64)
        .map(|abr| (abr * 100.0) as i64)
        .unwrap_or_default()
}

fn format_has_video(format: &Value) -> bool {
    codec_name_is_present(
        &format
            .get("vcodec")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase(),
    )
}

fn format_has_audio(format: &Value) -> bool {
    codec_name_is_present(
        &format
            .get("acodec")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase(),
    )
}

fn log_selected_format(label: &str, format: &Value) {
    info!(
        "yt-dlp: {label} format_id={} protocol={} ext={} container={} resolution={} vcodec={} acodec={} tbr={} abr={} audio_channels={} asr={} language={}",
        format
            .get("format_id")
            .and_then(Value::as_str)
            .unwrap_or("<top-level>"),
        format
            .get("protocol")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
        format
            .get("ext")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
        format
            .get("container")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
        format
            .get("resolution")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
        format
            .get("vcodec")
            .and_then(Value::as_str)
            .unwrap_or("<none>"),
        format
            .get("acodec")
            .and_then(Value::as_str)
            .unwrap_or("<none>"),
        format_number_for_log(format, "tbr"),
        format_number_for_log(format, "abr"),
        format_number_for_log(format, "audio_channels"),
        format_number_for_log(format, "asr"),
        format
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
    );
}

fn format_number_for_log(format: &Value, field: &str) -> String {
    format
        .get(field)
        .map(|value| match value {
            Value::Number(value) => value.to_string(),
            Value::String(value) => value.clone(),
            _ => "<unknown>".to_string(),
        })
        .unwrap_or_else(|| "<unknown>".to_string())
}

pub(crate) async fn try_resolve(app: &AppHandle, raw_url: &str) -> Option<ResolvedMedia> {
    match resolve(app, raw_url).await {
        Ok(resolved) => resolved,
        Err(error) => {
            warn!("yt-dlp: resolve failed for {}: {error}", redact_url(raw_url));
            None
        }
    }
}

fn run_ytdlp_command(
    ytdl_path: &str,
    proxy_url: Option<&str>,
    cookies_from_browser: Option<&str>,
    format_selector: &str,
    raw_url: &str,
) -> Result<std::process::Output, String> {
    let mut command = Command::new(ytdl_path);
    hide_windows_console(&mut command);
    let mut log_args = vec![
        "--dump-single-json".to_string(),
        "--no-playlist".to_string(),
        "-f".to_string(),
        format_selector.to_string(),
        redact_url(raw_url),
    ];
    command
        .arg("--dump-single-json")
        .arg("--no-playlist")
        .arg("-f")
        .arg(format_selector)
        .arg(raw_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(proxy_url) = proxy_url {
        command.arg("--proxy").arg(proxy_url);
        log_args.push("--proxy".to_string());
        log_args.push(redact_url(proxy_url));
    }

    if let Some(browser) = cookies_from_browser {
        command.arg("--cookies-from-browser").arg(browser);
        log_args.push("--cookies-from-browser".to_string());
        log_args.push(browser.to_string());
    }

    info!(
        "yt-dlp: run {}",
        format_command_for_log(ytdl_path, &log_args)
    );

    let mut child = command
        .spawn()
        .map_err(|error| format!("yt-dlp failed to start: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "yt-dlp stdout pipe is unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "yt-dlp stderr pipe is unavailable".to_string())?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started_at = Instant::now();
    let deadline = Instant::now() + YTDLP_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("yt-dlp wait failed: {error}"))?
        {
            let elapsed = started_at.elapsed();
            info!("yt-dlp: finished in {:.3}s", elapsed.as_secs_f64());
            let stdout = stdout_reader
                .join()
                .map_err(|_| "yt-dlp stdout reader panicked".to_string())?
                .map_err(|error| format!("yt-dlp stdout read failed: {error}"))?;
            let stderr = stderr_reader
                .join()
                .map_err(|_| "yt-dlp stderr reader panicked".to_string())?
                .map_err(|error| format!("yt-dlp stderr read failed: {error}"))?;
            let output = std::process::Output {
                status,
                stdout,
                stderr,
            };
            info!(
                "yt-dlp: exit status={} stdout={}B stderr={}B",
                output.status,
                output.stdout.len(),
                output.stderr.len()
            );
            return Ok(output);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            warn!(
                "yt-dlp: timed out after {:.3}s",
                started_at.elapsed().as_secs_f64()
            );
            return Err(format!("yt-dlp timed out after {}s", YTDLP_TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "windows")]
fn hide_windows_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_windows_console(_: &mut Command) {}

fn format_command_for_log(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn select_candidates(value: &Value) -> Vec<Candidate> {
    let top_headers = parse_headers(value.get("http_headers"));

    let requested_formats = select_requested_formats(value, &top_headers);
    if !requested_formats.is_empty() {
        return requested_formats;
    }

    if let Some(url) = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| is_http_url(url))
    {
        return vec![Candidate {
            url: url.to_string(),
            headers: top_headers,
            available_at: value.get("available_at").and_then(Value::as_i64),
            format_id: None,
            protocol: value
                .get("protocol")
                .and_then(Value::as_str)
                .map(str::to_string),
            resolution: value
                .get("resolution")
                .and_then(Value::as_str)
                .map(str::to_string),
            score: i64::MAX,
        }];
    }

    select_best_video_candidate(value, &top_headers)
        .or_else(|| select_best_combined_candidate(value, &top_headers))
        .into_iter()
        .collect()
}

fn select_requested_formats(value: &Value, top_headers: &[(String, String)]) -> Vec<Candidate> {
    value
        .get("requested_formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|format| format_candidate(format, top_headers))
        .collect()
}

fn select_best_video_candidate(value: &Value, top_headers: &[(String, String)]) -> Option<Candidate> {
    value
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|format| format_candidate(format, &top_headers))
        .filter(|candidate| candidate.score >= 10_000_000)
        .max_by_key(|candidate| candidate.score)
}

fn select_best_combined_candidate(value: &Value, top_headers: &[(String, String)]) -> Option<Candidate> {
    value
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|format| format_candidate(format, &top_headers))
        .filter(|candidate| candidate.score >= 3_000_000 && candidate.score < 10_000_000)
        .max_by_key(|candidate| candidate.score)
}

fn log_selected_candidate(label: &str, candidate: &Candidate) {
    info!(
        "yt-dlp: {label} format_id={} protocol={} resolution={} score={}",
        candidate.format_id.as_deref().unwrap_or("<top-level>"),
        candidate.protocol.as_deref().unwrap_or("<unknown>"),
        candidate.resolution.as_deref().unwrap_or("<unknown>"),
        candidate.score
    );
}

fn is_likely_live_candidate(candidate: &Candidate) -> bool {
    let protocol = candidate.protocol.as_deref().unwrap_or("").to_ascii_lowercase();
    let url = candidate.url.to_ascii_lowercase();
    protocol.contains("m3u8") || url.contains(".m3u8")
}

fn extract_media_title(value: &Value) -> Option<String> {
    value
        .get("title")
        .or_else(|| value.get("fulltitle"))
        .and_then(Value::as_str)
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn format_candidate(format: &Value, top_headers: &[(String, String)]) -> Option<Candidate> {
    let url = format.get("url").and_then(Value::as_str)?;
    if !is_http_url(url) {
        return None;
    }
    if !is_playable_format(format) {
        return None;
    }

    let headers = merge_headers(top_headers, &parse_headers(format.get("http_headers")));
    Some(Candidate {
        url: url.to_string(),
        headers,
        available_at: format.get("available_at").and_then(Value::as_i64),
        format_id: format
            .get("format_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        protocol: format
            .get("protocol")
            .and_then(Value::as_str)
            .map(str::to_string),
        resolution: format
            .get("resolution")
            .and_then(Value::as_str)
            .map(str::to_string),
        score: score_format(format, url),
    })
}

fn is_playable_format(format: &Value) -> bool {
    let protocol = format
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ext = format
        .get("ext")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let vcodec = format
        .get("vcodec")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let acodec = format
        .get("acodec")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches!(ext.as_str(), "mhtml" | "jpg" | "webp" | "png") {
        return false;
    }
    if matches!(protocol.as_str(), "mhtml" | "images") {
        return false;
    }
    codec_name_is_present(&vcodec) || codec_name_is_present(&acodec)
}

fn score_format(format: &Value, url: &str) -> i64 {
    let protocol = format
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ext = format
        .get("ext")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let vcodec = format
        .get("vcodec")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let acodec = format
        .get("acodec")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let height = format.get("height").and_then(Value::as_i64).unwrap_or(0);
    let tbr = format
        .get("tbr")
        .and_then(Value::as_f64)
        .map(|value| value as i64)
        .unwrap_or(0);

    let has_video = codec_name_is_present(&vcodec);
    let has_audio = codec_name_is_present(&acodec);
    let is_hls = protocol.contains("m3u8") || url.to_ascii_lowercase().contains(".m3u8");
    let is_direct_https = protocol == "https";
    let mut score = 0;
    if has_video && !has_audio {
        score += 10_000_000;
    } else if has_video && has_audio {
        score += 3_000_000;
    } else if has_audio && !has_video {
        score += 100_000;
    }

    if height > 0 && height <= 1080 {
        score += height * 10_000;
    } else if height > 1080 {
        score -= 1_000_000 + height * 1_000;
    }
    if is_direct_https {
        score += 50_000;
    } else if is_hls {
        score += 25_000;
    }
    if matches!(ext.as_str(), "mp4" | "m4a" | "webm") {
        score += 20_000;
    }
    score + height * 100 + tbr
}

fn codec_name_is_present(value: &str) -> bool {
    !value.is_empty() && value != "none"
}

fn parse_headers(value: Option<&Value>) -> Vec<(String, String)> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|headers| headers.iter())
        .filter_map(|(name, value)| {
            value
                .as_str()
                .map(|value| (name.to_string(), value.to_string()))
        })
        .collect()
}

fn merge_headers(
    base: &[(String, String)],
    override_headers: &[(String, String)],
) -> Vec<(String, String)> {
    let mut merged = base.to_vec();
    for (name, value) in override_headers {
        if let Some((_, existing_value)) = merged
            .iter_mut()
            .find(|(existing_name, _)| existing_name.eq_ignore_ascii_case(name))
        {
            *existing_value = value.clone();
        } else {
            merged.push((name.clone(), value.clone()));
        }
    }
    merged
}

fn is_http_url(raw: &str) -> bool {
    Url::parse(raw)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

fn cache_cast_resolution(raw_url: &str, streams: ResolvedCastStreams) {
    let Ok(mut cache) = YTDLP_CAST_RESOLUTION_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    else {
        return;
    };
    cache.insert(
        raw_url.to_string(),
        CachedCastResolution {
            resolved_at: Instant::now(),
            streams,
        },
    );
    cache.retain(|_, result| result.resolved_at.elapsed() <= YTDLP_RESOLUTION_CACHE_TTL);
}

fn cached_cast_resolution(raw_url: &str) -> Option<ResolvedCastStreams> {
    let mut cache = YTDLP_CAST_RESOLUTION_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?;
    let result = cache.get(raw_url)?;
    if result.resolved_at.elapsed() > YTDLP_RESOLUTION_CACHE_TTL {
        cache.remove(raw_url);
        return None;
    }
    Some(result.streams.clone())
}

fn is_likely_direct_stream_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    DIRECT_STREAM_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(&format!(".{extension}")))
}

fn is_cookie_permission_error(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    text.contains("could not copy cookies")
        || text.contains("permission denied")
        || text.contains("failed to decrypt")
        || text.contains("could not read cookies")
        || text.contains("unable to get cookies")
        || (text.contains("cookie") && text.contains("error"))
}

fn redact_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_string();
    };
    if !url.username().is_empty() {
        let _ = url.set_username("<user>");
        let _ = url.set_password(Some("<redacted>"));
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        cache_cast_resolution, cached_cast_resolution, format_candidate, select_candidates,
        select_cast_streams, ResolvedCastStreams,
    };
    use serde_json::json;

    #[test]
    fn cast_prefers_combined_stream_at_requested_height() {
        let value = json!({
            "requested_formats": [
                { "format_id": "137", "url": "https://example.com/video", "vcodec": "avc1", "acodec": "none", "height": 1080 },
                { "format_id": "251", "url": "https://example.com/opus", "vcodec": "none", "acodec": "opus" }
            ],
            "formats": [
                { "format_id": "137", "url": "https://example.com/video", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 1080, "tbr": 3000 },
                { "format_id": "251", "url": "https://example.com/opus", "protocol": "https", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160 },
                { "format_id": "22", "url": "https://example.com/combined", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a.40.2", "height": 1080, "tbr": 1800 }
            ]
        });

        let streams = select_cast_streams(&value, 1080).expect("cast streams");
        assert!(matches!(
            streams,
            ResolvedCastStreams::Single { url, .. }
                if url == "https://example.com/combined"
        ));
    }

    #[test]
    fn cast_remuxes_instead_of_lowering_the_requested_resolution() {
        let value = json!({
            "requested_formats": [
                { "format_id": "137", "url": "https://example.com/video", "vcodec": "avc1", "acodec": "none", "height": 1080 },
                { "format_id": "251", "url": "https://example.com/opus", "vcodec": "none", "acodec": "opus" }
            ],
            "formats": [
                { "format_id": "137", "url": "https://example.com/video", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 1080, "tbr": 3000 },
                { "format_id": "251", "url": "https://example.com/opus", "protocol": "https", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160 },
                { "format_id": "22", "url": "https://example.com/combined", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a.40.2", "height": 720, "tbr": 1800 }
            ]
        });

        let streams = select_cast_streams(&value, 1080).expect("cast streams");
        assert!(matches!(
            streams,
            ResolvedCastStreams::VideoAudio { video_url, audio_url, .. }
                if video_url == "https://example.com/video" && audio_url == "https://example.com/opus"
        ));
    }

    #[test]
    fn cast_remux_fallback_prefers_aac_audio() {
        let value = json!({
            "requested_formats": [
                { "format_id": "137", "url": "https://example.com/video", "vcodec": "avc1", "acodec": "none", "height": 1080, "available_at": 1_789_000_005 },
                { "format_id": "251", "url": "https://example.com/opus", "vcodec": "none", "acodec": "opus" }
            ],
            "formats": [
                { "format_id": "137", "url": "https://example.com/video", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 1080 },
                { "format_id": "251", "url": "https://example.com/opus", "protocol": "https", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160 },
                { "format_id": "140", "url": "https://example.com/aac", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128, "available_at": 1_789_000_010 }
            ]
        });

        let streams = select_cast_streams(&value, 1080).expect("cast streams");
        assert!(matches!(
            streams,
            ResolvedCastStreams::VideoAudio {
                video_url,
                audio_url,
                video_available_at,
                audio_available_at,
                ..
            } if video_url == "https://example.com/video"
                && audio_url == "https://example.com/aac"
                && video_available_at == Some(1_789_000_005)
                && audio_available_at == Some(1_789_000_010)
        ));
    }

    #[test]
    fn cache_keeps_only_the_structured_cast_descriptor() {
        let raw_url = "https://example.com/watch/cache-test";
        cache_cast_resolution(
            raw_url,
            ResolvedCastStreams::Single {
                url: "https://cdn.example.com/stream".to_string(),
                headers: vec![("User-Agent".to_string(), "test".to_string())],
            },
        );

        let streams = cached_cast_resolution(raw_url).expect("cached streams");
        assert!(matches!(
            streams,
            ResolvedCastStreams::Single { url, headers, .. }
                if url == "https://cdn.example.com/stream" && headers.len() == 1
        ));
    }

    #[test]
    fn format_candidate_preserves_ytdlp_source_availability_time() {
        let format = json!({
            "format_id": "137",
            "url": "https://example.com/video",
            "protocol": "https",
            "ext": "mp4",
            "vcodec": "avc1",
            "acodec": "none",
            "height": 1080,
            "available_at": 1_789_000_005,
        });
        let candidate = format_candidate(&format, &[]).expect("playable candidate");
        assert_eq!(candidate.available_at, Some(1_789_000_005));
    }

    #[test]
    fn requested_formats_remain_separate_resolved_streams() {
        let value = json!({
            "requested_formats": [
                { "format_id": "137", "url": "https://example.com/video", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 1080 },
                { "format_id": "251", "url": "https://example.com/audio", "protocol": "https", "ext": "webm", "vcodec": "none", "acodec": "opus" }
            ]
        });

        let streams = select_candidates(&value);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].url, "https://example.com/video");
        assert_eq!(streams[1].url, "https://example.com/audio");
    }
}
