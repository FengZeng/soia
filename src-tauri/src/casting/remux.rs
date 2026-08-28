use log::{info, warn};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

const CMAF_SEGMENT_DURATION_SECONDS: u32 = 2;
const CMAF_READY_TIMEOUT: Duration = Duration::from_secs(15);
const CMAF_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CMAF_TEMP_DIR_PREFIX: &str = "soia-cast-cmaf-";
const CMAF_TEMP_DIR_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);
const PLAYLIST_FILE_NAME: &str = "playlist.m3u8";
const INIT_FILE_NAME: &str = "init.mp4";
const SEGMENT_FILE_PREFIX: &str = "segment-";

// `video/mp2t` is the registered MPEG transport-stream type, but the verified DLNA renderer
// rejects it for this progressive remux path and accepts `video/mpeg`.
pub(crate) const MPEGTS_MIME_TYPE: &str = "video/mpeg";
pub(crate) const FMP4_MIME_TYPE: &str = "video/mp4";

/// Direct, single-URL remux formats for receivers that cannot consume separate DASH streams.
/// CMAF/HLS remains a distinct experimental transport because it requires playlist support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProgressiveRemuxFormat {
    MpegTs,
    FragmentedMp4,
}

impl ProgressiveRemuxFormat {
    pub(crate) fn selected_for_cast() -> Self {
        match std::env::var("SOIA_CAST_REMUX_FORMAT") {
            Ok(value) => Self::from_setting(&value).unwrap_or_else(|| {
                warn!(
                    "casting: unknown SOIA_CAST_REMUX_FORMAT={value:?}; using MPEG-TS"
                );
                Self::MpegTs
            }),
            Err(_) => Self::MpegTs,
        }
        // Self::FragmentedMp4
    }

    fn from_setting(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mpegts" | "mpeg-ts" | "ts" => Some(Self::MpegTs),
            "fmp4" | "fragmented-mp4" | "fragmented_mp4" => Some(Self::FragmentedMp4),
            _ => None,
        }
    }

    pub(crate) fn mime_type(self) -> &'static str {
        match self {
            Self::MpegTs => MPEGTS_MIME_TYPE,
            Self::FragmentedMp4 => FMP4_MIME_TYPE,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MpegTs => "MPEG-TS",
            Self::FragmentedMp4 => "fragmented MP4",
        }
    }

    fn backend_label(self) -> &'static str {
        match self {
            Self::MpegTs => "mpegts-remux",
            Self::FragmentedMp4 => "fmp4-remux",
        }
    }
}

/// A compatibility-first cast transport. Unlike HLS, it needs no playlist support from the
/// receiver and forwards a single progressive remux stream directly from the FFmpeg API.
pub(crate) struct ProgressiveRemuxBackend {
    origin: String,
    input: CmafInput,
    output_format: ProgressiveRemuxFormat,
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    download_speed_meter: crate::media_gateway::DownloadSpeedMeterHandle,
}

impl ProgressiveRemuxBackend {
    pub(crate) fn new(
        output_format: ProgressiveRemuxFormat,
        origin: String,
        video_gateway_url: String,
        audio_gateway_url: String,
    ) -> Self {
        Self {
            origin,
            input: CmafInput {
                video_url: video_gateway_url,
                audio_url: audio_gateway_url,
            },
            output_format,
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
        info!(
            "casting: preparing {} remux via media gateway, map=0:v:0+1:a:0 origin={}",
            self.output_format.label(),
            redact_stream_url(&self.origin),
        );
        write_progressive_remux_headers(stream, self.output_format).await?;
        let (packets, mut receiver) = tokio::sync::mpsc::channel(8);
        let input = ffmpeg_remux_input(&self.input);
        let output_format = match self.output_format {
            ProgressiveRemuxFormat::MpegTs => crate::ffmpeg::remux::ProgressiveFormat::MpegTs,
            ProgressiveRemuxFormat::FragmentedMp4 => crate::ffmpeg::remux::ProgressiveFormat::FragmentedMp4,
        };
        let cancelled = self.cancelled.clone();
        let producer = tokio::task::spawn_blocking(move || {
            crate::ffmpeg::remux::remux_progressive(input, output_format, cancelled, packets)
        });
        info!("casting: FFmpeg API {} stream started", self.output_format.label());
        loop {
            tokio::select! {
                _ = self.cancel_notify.notified() => {
                    drop(receiver);
                    let _ = producer.await;
                    return Ok(());
                }
                packet = receiver.recv() => match packet {
                    Some(packet) => stream.write_all(&packet).await.map_err(|error| error.to_string())?,
                    None => break,
                },
            }
        }
        producer
            .await
            .map_err(|error| format!("FFmpeg remux worker failed: {error}"))??;
        info!("casting: FFmpeg API {} stream finished", self.output_format.label());
        Ok(())
    }
}

fn redact_stream_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return "<invalid-url>".to_string();
    };
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

impl crate::media_gateway::MediaSourceBackend for ProgressiveRemuxBackend {
    fn label(&self) -> &'static str {
        self.output_format.backend_label()
    }

    fn origin(&self) -> &str {
        &self.origin
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
                write_progressive_remux_headers(stream, self.output_format).await
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
    origin: String,
    input: CmafInput,
    output_dir: PathBuf,
    started: AtomicBool,
    cancelled: Arc<AtomicBool>,
    state: Mutex<CmafProducerState>,
    ready_notify: Notify,
    download_speed_meter: crate::media_gateway::DownloadSpeedMeterHandle,
}

#[derive(Clone)]
struct CmafInput {
    video_url: String,
    audio_url: String,
}

fn ffmpeg_remux_input(input: &CmafInput) -> crate::ffmpeg::remux::RemuxInput {
    crate::ffmpeg::remux::RemuxInput {
        video: crate::ffmpeg::remux::StreamInput {
            url: input.video_url.clone(),
        },
        audio: crate::ffmpeg::remux::StreamInput {
            url: input.audio_url.clone(),
        },
    }
}

enum CmafProducerState {
    Pending,
    Running,
    Finished,
    Failed(String),
}

impl HlsCmafSession {
    pub(crate) fn new(
        origin: String,
        video_gateway_url: String,
        audio_gateway_url: String,
    ) -> Result<Arc<Self>, String> {
        remove_stale_cmaf_directories();
        let output_dir = std::env::temp_dir().join(format!(
            "{CMAF_TEMP_DIR_PREFIX}{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&output_dir)
            .map_err(|error| format!("could not create CMAF temp directory: {error}"))?;
        Ok(Arc::new(Self {
            origin,
            input: CmafInput {
                video_url: video_gateway_url,
                audio_url: audio_gateway_url,
            },
            output_dir,
            started: AtomicBool::new(false),
            cancelled: Arc::new(AtomicBool::new(false)),
            state: Mutex::new(CmafProducerState::Pending),
            ready_notify: Notify::new(),
            download_speed_meter: crate::media_gateway::new_download_speed_meter(),
        }))
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
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

/// The media-gateway-facing session remains independent of transport details; this producer uses
/// the same in-process FFmpeg API as progressive remuxing.
async fn run_cmaf_producer(session: Arc<HlsCmafSession>) -> Result<(), String> {
    if session.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let input = ffmpeg_remux_input(&session.input);
    let output_dir = session.output_dir.clone();
    let cancelled = session.cancelled.clone();
    info!("casting: FFmpeg API CMAF producer started");
    let producer = tokio::task::spawn_blocking(move || {
        crate::ffmpeg::remux::remux_hls(
            input,
            &output_dir,
            CMAF_SEGMENT_DURATION_SECONDS,
            cancelled,
        )
    });
    let result = producer
        .await
        .map_err(|error| format!("FFmpeg CMAF worker failed: {error}"))?;
    if result.is_err() && session.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    result?;
    info!("casting: FFmpeg API CMAF producer finished");
    Ok(())
}

async fn write_progressive_remux_headers(
    stream: &mut tokio::net::TcpStream,
    output_format: ProgressiveRemuxFormat,
) -> Result<(), String> {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nConnection: close\r\nAccept-Ranges: none\r\nCache-Control: no-cache\r\n\r\n",
        output_format.mime_type(),
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::{
        ProgressiveRemuxFormat, FMP4_MIME_TYPE, MPEGTS_MIME_TYPE,
    };

    #[test]
    fn uses_the_verified_renderer_compatible_mpegts_mime_type() {
        assert_eq!(MPEGTS_MIME_TYPE, "video/mpeg");
        assert_eq!(
            ProgressiveRemuxFormat::FragmentedMp4.mime_type(),
            FMP4_MIME_TYPE
        );
    }

    #[test]
    fn parses_progressive_remux_format_settings() {
        assert_eq!(
            ProgressiveRemuxFormat::from_setting("mpegts"),
            Some(ProgressiveRemuxFormat::MpegTs)
        );
        assert_eq!(
            ProgressiveRemuxFormat::from_setting("fmp4"),
            Some(ProgressiveRemuxFormat::FragmentedMp4)
        );
        assert_eq!(ProgressiveRemuxFormat::from_setting("hls"), None);
    }
}
