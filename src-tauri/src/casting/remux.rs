use log::{info, warn};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Notify;

const CMAF_SEGMENT_DURATION_SECONDS: u32 = 2;
const CMAF_READY_TIMEOUT: Duration = Duration::from_secs(15);
const CMAF_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CMAF_TEMP_DIR_PREFIX: &str = "soia-cast-cmaf-";
const CMAF_TEMP_DIR_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);
const PLAYLIST_FILE_NAME: &str = "playlist.m3u8";
const INIT_FILE_NAME: &str = "init.mp4";
const SEGMENT_FILE_PREFIX: &str = "segment-";

pub(crate) const MPEGTS_MIME_TYPE: &str = "video/mpeg";

/// The compatibility-first cast transport. Unlike HLS, it needs no playlist support from the
/// receiver and forwards muxed MPEG-TS immediately after ffmpeg starts.
pub(crate) struct MpegTsRemuxBackend {
    input: CmafInput,
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    download_speed_meter: crate::media_gateway::DownloadSpeedMeterHandle,
}

impl MpegTsRemuxBackend {
    pub(crate) fn new(
        video_url: String,
        audio_url: String,
        video_headers: Vec<(String, String)>,
        audio_headers: Vec<(String, String)>,
    ) -> Self {
        Self {
            input: CmafInput {
                video_url,
                audio_url,
                video_headers,
                audio_headers,
            },
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
            download_speed_meter: crate::media_gateway::new_download_speed_meter(),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_notify.notify_waiters();
    }

    async fn stream(&self, stream: &mut tokio::net::TcpStream) -> Result<(), String> {
        let ffmpeg_path = resolve_ffmpeg_path()
            .ok_or_else(|| "ffmpeg is not available for MPEG-TS cast remuxing".to_string())?;
        let mut child = tokio::process::Command::new(ffmpeg_path)
            .args(build_mpegts_args(&self.input))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("ffmpeg failed to start: {error}"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "ffmpeg stdout pipe is unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "ffmpeg stderr pipe is unavailable".to_string())?;
        let diagnostics = tokio::spawn(collect_ffmpeg_diagnostics(stderr));
        write_mpegts_headers(stream).await?;
        info!("casting: ffmpeg MPEG-TS stream started");
        let mut buffer = vec![0_u8; 256 * 1024];
        let result = loop {
            if self.cancelled.load(Ordering::Acquire) {
                let _ = child.start_kill();
                break Ok(());
            }
            let read = tokio::select! {
                _ = self.cancel_notify.notified() => {
                    let _ = child.start_kill();
                    break Ok(());
                }
                read = stdout.read(&mut buffer) => read,
            };
            let bytes = read.map_err(|error| error.to_string())?;
            if bytes == 0 {
                break Ok(());
            }
            if let Err(error) = stream.write_all(&buffer[..bytes]).await {
                break Err(error.to_string());
            }
        };
        let status = child
            .wait()
            .await
            .map_err(|error| format!("ffmpeg wait failed: {error}"))?;
        let diagnostics = diagnostics.await.unwrap_or_default();
        if result.is_err() {
            return result;
        }
        if !status.success() && !self.cancelled.load(Ordering::Acquire) {
            let detail = if diagnostics.is_empty() {
                "no diagnostics".to_string()
            } else {
                diagnostics.join("; ")
            };
            return Err(format!("ffmpeg exited with status {status}: {detail}"));
        }
        info!("casting: ffmpeg MPEG-TS stream finished");
        Ok(())
    }
}

impl crate::media_gateway::MediaSourceBackend for MpegTsRemuxBackend {
    fn label(&self) -> &'static str {
        "mpegts-remux"
    }

    fn origin(&self) -> &str {
        &self.input.video_url
    }

    fn download_speed_meter(&self) -> &crate::media_gateway::DownloadSpeedMeterHandle {
        &self.download_speed_meter
    }

    fn shutdown(&self) {
        self.cancel();
    }

    fn handle<'a>(
        &'a self,
        _app_handle: Option<&'a tauri::AppHandle>,
        stream: &'a mut tokio::net::TcpStream,
        method: &'a str,
        _range: Option<&'a str>,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if method == "HEAD" {
                write_mpegts_headers(stream).await
            } else {
                self.stream(stream).await
            }
        })
    }
}

/// The media-gateway-facing CMAF session contract. It deliberately knows nothing about HTTP or
/// DLNA: replacing the command-line producer with an FFmpeg API implementation only changes
/// `run_cmaf_producer`, while the cast URLs and lease lifecycle stay untouched.
pub(crate) struct HlsCmafSession {
    input: CmafInput,
    output_dir: PathBuf,
    started: AtomicBool,
    cancelled: AtomicBool,
    cancel_notify: Notify,
    state: Mutex<CmafProducerState>,
    ready_notify: Notify,
    download_speed_meter: crate::media_gateway::DownloadSpeedMeterHandle,
}

#[derive(Clone)]
struct CmafInput {
    video_url: String,
    audio_url: String,
    video_headers: Vec<(String, String)>,
    audio_headers: Vec<(String, String)>,
}

enum CmafProducerState {
    Pending,
    Running,
    Finished,
    Failed(String),
}

impl HlsCmafSession {
    pub(crate) fn new(
        video_url: String,
        audio_url: String,
        video_headers: Vec<(String, String)>,
        audio_headers: Vec<(String, String)>,
    ) -> Result<Arc<Self>, String> {
        remove_stale_cmaf_directories();
        let output_dir = std::env::temp_dir().join(format!(
            "{CMAF_TEMP_DIR_PREFIX}{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&output_dir)
            .map_err(|error| format!("could not create CMAF temp directory: {error}"))?;
        Ok(Arc::new(Self {
            input: CmafInput {
                video_url,
                audio_url,
                video_headers,
                audio_headers,
            },
            output_dir,
            started: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            cancel_notify: Notify::new(),
            state: Mutex::new(CmafProducerState::Pending),
            ready_notify: Notify::new(),
            download_speed_meter: crate::media_gateway::new_download_speed_meter(),
        }))
    }

    pub(crate) fn origin(&self) -> &str {
        &self.input.video_url
    }

    pub(crate) fn download_speed_meter(&self) -> &crate::media_gateway::DownloadSpeedMeterHandle {
        &self.download_speed_meter
    }

    pub(crate) fn start(self: &Arc<Self>) {
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let session = self.clone();
        tokio::spawn(async move {
            session.set_state(CmafProducerState::Running);
            let result = run_cmaf_producer(session.clone()).await;
            match result {
                Ok(()) if session.cancelled.load(Ordering::Acquire) => {
                    let _ = std::fs::remove_dir_all(&session.output_dir);
                }
                Ok(()) => session.set_state(CmafProducerState::Finished),
                Err(_error) if session.cancelled.load(Ordering::Acquire) => {
                    let _ = std::fs::remove_dir_all(&session.output_dir);
                }
                Err(error) => session.set_state(CmafProducerState::Failed(error)),
            }
            session.ready_notify.notify_waiters();
        });
    }

    pub(crate) async fn wait_until_ready(self: &Arc<Self>) -> Result<(), String> {
        self.start();
        let deadline = Instant::now() + CMAF_READY_TIMEOUT;
        loop {
            if self.playlist_is_ready() {
                return Ok(());
            }
            if let Some(error) = self.failure() {
                return Err(error);
            }
            if self.cancelled.load(Ordering::Acquire) {
                return Err("CMAF cast session was cancelled".to_string());
            }
            if Instant::now() >= deadline {
                return Err("CMAF playlist did not become ready in time".to_string());
            }
            tokio::select! {
                _ = self.ready_notify.notified() => {}
                _ = tokio::time::sleep(CMAF_READY_POLL_INTERVAL) => {}
            }
        }
    }

    pub(crate) fn playlist(&self) -> Result<String, String> {
        std::fs::read_to_string(self.output_dir.join(PLAYLIST_FILE_NAME))
            .map_err(|error| format!("CMAF playlist is unavailable: {error}"))
    }

    /// Only FFmpeg-generated init and completed CMAF segment files can become cast resources.
    pub(crate) fn resource_path(&self, name: &str) -> Option<PathBuf> {
        let path = Path::new(name);
        if path.file_name()?.to_str()? != name {
            return None;
        }
        let allowed = name == INIT_FILE_NAME
            || (name.starts_with(SEGMENT_FILE_PREFIX) && name.ends_with(".m4s"));
        if !allowed {
            return None;
        }
        let path = self.output_dir.join(name);
        path.is_file().then_some(path)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_notify.notify_waiters();
        self.ready_notify.notify_waiters();
        let producer_is_running = self
            .state
            .lock()
            .map(|state| matches!(*state, CmafProducerState::Running))
            .unwrap_or(true);
        if !producer_is_running {
            let _ = std::fs::remove_dir_all(&self.output_dir);
        }
    }

    fn playlist_is_ready(&self) -> bool {
        let Ok(playlist) = self.playlist() else {
            return false;
        };
        playlist.contains("#EXTINF:") && self.resource_path(INIT_FILE_NAME).is_some()
    }

    fn failure(&self) -> Option<String> {
        self.state.lock().ok().and_then(|state| match &*state {
            CmafProducerState::Failed(error) => Some(error.clone()),
            _ => None,
        })
    }

    fn set_state(&self, next: CmafProducerState) {
        if let Ok(mut state) = self.state.lock() {
            *state = next;
        }
    }
}

/// Current producer implementation. This is the only boundary that needs replacing when the
/// project moves to an FFmpeg API; `HlsCmafSession` still exposes the same files and lifecycle.
async fn run_cmaf_producer(session: Arc<HlsCmafSession>) -> Result<(), String> {
    if session.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let ffmpeg_path = resolve_ffmpeg_path()
        .ok_or_else(|| "ffmpeg is not available for CMAF cast remuxing".to_string())?;
    let args = build_cmaf_args(&session.input, &session.output_dir);
    let mut child = tokio::process::Command::new(ffmpeg_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("ffmpeg failed to start: {error}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ffmpeg stderr pipe is unavailable".to_string())?;
    let diagnostics = tokio::spawn(collect_ffmpeg_diagnostics(stderr));
    info!("casting: ffmpeg CMAF producer started");
    let cancelled = session.cancel_notify.notified();
    tokio::pin!(cancelled);
    let status = if session.cancelled.load(Ordering::Acquire) {
        let _ = child.start_kill();
        child.wait().await.map_err(|error| format!("ffmpeg wait failed: {error}"))?
    } else {
        tokio::select! {
            _ = &mut cancelled => {
                let _ = child.start_kill();
                child.wait().await.map_err(|error| format!("ffmpeg wait failed: {error}"))?
            }
            status = child.wait() => status.map_err(|error| format!("ffmpeg wait failed: {error}"))?,
        }
    };
    let diagnostics = diagnostics.await.unwrap_or_default();
    if !status.success() && !session.cancelled.load(Ordering::Acquire) {
        let detail = if diagnostics.is_empty() {
            "no diagnostics".to_string()
        } else {
            diagnostics.join("; ")
        };
        return Err(format!("ffmpeg exited with status {status}: {detail}"));
    }
    info!("casting: ffmpeg CMAF producer finished");
    Ok(())
}

fn build_cmaf_args(input: &CmafInput, output_dir: &Path) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "warning".to_string(),
        "-nostdin".to_string(),
    ];
    append_input_args(&mut args, &input.video_url, &input.video_headers);
    append_input_args(&mut args, &input.audio_url, &input.audio_headers);
    args.extend(
        [
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "copy",
            "-f",
            "hls",
            "-hls_time",
        ]
        .into_iter()
        .map(str::to_string),
    );
    args.push(CMAF_SEGMENT_DURATION_SECONDS.to_string());
    args.extend(
        [
            "-hls_list_size",
            "0",
            "-hls_segment_type",
            "fmp4",
            "-hls_fmp4_init_filename",
            INIT_FILE_NAME,
            "-hls_segment_filename",
        ]
        .into_iter()
        .map(str::to_string),
    );
    args.push(
        output_dir
            .join("segment-%05d.m4s")
            .to_string_lossy()
            .to_string(),
    );
    args.extend(
        ["-hls_flags", "independent_segments+temp_file"]
            .into_iter()
            .map(str::to_string),
    );
    args.push(
        output_dir
            .join(PLAYLIST_FILE_NAME)
            .to_string_lossy()
            .to_string(),
    );
    args
}

fn build_mpegts_args(input: &CmafInput) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "warning".to_string(),
        "-nostdin".to_string(),
    ];
    append_input_args(&mut args, &input.video_url, &input.video_headers);
    append_input_args(&mut args, &input.audio_url, &input.audio_headers);
    args.extend(
        [
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "copy",
            "-f",
            "mpegts",
            "-flush_packets",
            "1",
            "pipe:1",
        ]
        .into_iter()
        .map(str::to_string),
    );
    args
}

fn append_input_args(args: &mut Vec<String>, url: &str, headers: &[(String, String)]) {
    args.push("-user_agent".to_string());
    args.push(user_agent(headers));
    if let Some(header_block) = header_option_value(headers) {
        args.push("-headers".to_string());
        args.push(header_block);
    }
    args.push("-i".to_string());
    args.push(url.to_string());
}

fn header_option_value(headers: &[(String, String)]) -> Option<String> {
    let rendered: String = headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("user-agent"))
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    (!rendered.is_empty()).then_some(rendered)
}

fn user_agent(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| "Lavf/61.7.100".to_string())
}

async fn write_mpegts_headers(stream: &mut tokio::net::TcpStream) -> Result<(), String> {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {MPEGTS_MIME_TYPE}\r\nConnection: close\r\nAccept-Ranges: none\r\nCache-Control: no-cache\r\n\r\n"
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(|error| error.to_string())
}

async fn collect_ffmpeg_diagnostics(stderr: tokio::process::ChildStderr) -> Vec<String> {
    let mut lines = BufReader::new(stderr).lines();
    let mut collected = Vec::new();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        warn!("ffmpeg: {trimmed}");
        collected.push(trimmed.to_string());
        if collected.len() > 8 {
            collected.remove(0);
        }
    }
    collected
}

fn remove_stale_cmaf_directories() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_cmaf_dir = path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(CMAF_TEMP_DIR_PREFIX));
        if !is_cmaf_dir {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map(|modified| modified.elapsed().unwrap_or_default() > CMAF_TEMP_DIR_MAX_AGE)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(target_os = "macos")]
const FFMPEG_DEFAULT_PATHS: &[&str] = &["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"];
#[cfg(not(target_os = "macos"))]
const FFMPEG_DEFAULT_PATHS: &[&str] = &["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"];

fn resolve_ffmpeg_path() -> Option<String> {
    if let Some(configured) = std::env::var("SOIA_FFMPEG_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        if Path::new(&configured).is_file() {
            return Some(configured);
        }
    }
    FFMPEG_DEFAULT_PATHS
        .iter()
        .find(|candidate| Path::new(candidate).is_file())
        .map(|candidate| (*candidate).to_string())
        .or_else(|| Some("ffmpeg".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{build_cmaf_args, build_mpegts_args, CmafInput, CMAF_SEGMENT_DURATION_SECONDS, MPEGTS_MIME_TYPE};
    use std::path::Path;

    #[test]
    fn builds_copy_only_fmp4_hls_output_with_per_input_headers() {
        let args = build_cmaf_args(
            &CmafInput {
                video_url: "https://cdn.example/video.m4s".to_string(),
                audio_url: "https://cdn.example/audio.m4s".to_string(),
                video_headers: vec![("Referer".to_string(), "https://www.bilibili.com/".to_string())],
                audio_headers: vec![("Cookie".to_string(), "SESSDATA=redacted".to_string())],
            },
            Path::new("/tmp/cmaf"),
        );
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "copy"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:a", "copy"]));
        assert!(args.windows(2).any(|pair| pair == ["-f", "hls"]));
        assert!(args.windows(2).any(|pair| pair == ["-hls_segment_type", "fmp4"]));
        assert!(args.windows(2).any(|pair| pair == ["-hls_time", &CMAF_SEGMENT_DURATION_SECONDS.to_string()]));
        assert!(args.iter().any(|arg| arg.contains("Referer: https://www.bilibili.com/")));
        assert!(args.iter().any(|arg| arg.contains("Cookie: SESSDATA=redacted")));
    }

    #[test]
    fn builds_copy_only_mpegts_output() {
        let args = build_mpegts_args(&CmafInput {
            video_url: "https://cdn.example/video.m4s".to_string(),
            audio_url: "https://cdn.example/audio.m4s".to_string(),
            video_headers: Vec::new(),
            audio_headers: Vec::new(),
        });
        assert!(args.windows(2).any(|pair| pair == ["-f", "mpegts"]));
        assert!(args.windows(2).any(|pair| pair == ["-flush_packets", "1"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "copy"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:a", "copy"]));
    }

    #[test]
    fn uses_the_verified_renderer_compatible_mpegts_mime_type() {
        assert_eq!(MPEGTS_MIME_TYPE, "video/mpeg");
    }
}
