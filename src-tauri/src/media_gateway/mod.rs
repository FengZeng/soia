mod lease;

use lease::{CastMediaLease, CastMediaLeaseRegistry, ResourceRegistration};

use log::{debug, info, warn};
use percent_encoding::percent_decode_str;
use reqwest::header::{
    HeaderName, HeaderValue, ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_TYPE, RANGE, USER_AGENT,
};
use futures_util::future::BoxFuture;
use futures_util::stream::{FuturesUnordered, StreamExt};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, TcpListener as StdTcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

const HTTP_USER_AGENT: &str = "Lavf/61.7.100";
const MAX_REQUEST_HEADER_BYTES: usize = 128 * 1024;
const FETCH_REMOTE_MAX_RETRIES: usize = 2;
const FETCH_REMOTE_RETRY_DELAY: Duration = Duration::from_millis(500);
const PARALLEL_RANGE_MIN_BYTES: u64 = 16 * 1024 * 1024;
const PARALLEL_RANGE_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
const PARALLEL_RANGE_CONNECTIONS: usize = 3;
const PARALLEL_RANGE_SETTING_LABEL: &str = "NETWORK_PARALLEL_DOWNLOAD";
const SMB_STREAM_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const SMB_PIPELINE_DEPTH: usize = 4;
const MEDIA_SOURCE_IDLE_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const MEDIA_SOURCE_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const MEDIA_SOURCE_MAX_ENTRIES: usize = 4096;
const MEDIA_SOURCE_TARGET_ENTRIES: usize = 3072;
const DOWNLOAD_SPEED_WINDOW: Duration = Duration::from_secs(2);
const DOWNLOAD_SPEED_STALE_AFTER: Duration = Duration::from_secs(2);
const DOWNLOAD_SPEED_MIN_SAMPLE_DURATION: Duration = Duration::from_millis(250);
const LOCAL_FILE_READ_CHUNK_BYTES: usize = 256 * 1024;
#[allow(dead_code)]
const CAST_MEDIA_LEASE_TTL: Duration = Duration::from_secs(2 * 60 * 60);
const CAST_MEDIA_LEASE_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

type BasicAuth = (String, String);
pub(crate) type ProxyHeaders = Vec<(String, String)>;

static LOOPBACK_MEDIA_BASE_URL: OnceLock<String> = OnceLock::new();
static MEDIA_SOURCE_BASIC_AUTH: OnceLock<Mutex<HashMap<String, BasicAuth>>> = OnceLock::new();
static MEDIA_SOURCE_HEADERS: OnceLock<Mutex<HashMap<String, ProxyHeaders>>> = OnceLock::new();
static MEDIA_GATEWAY_CLIENT: OnceLock<Mutex<Option<CachedClient>>> = OnceLock::new();
static MEDIA_GATEWAY_PARALLEL_RANGE_ENABLED: AtomicBool = AtomicBool::new(false);
static MEDIA_SOURCE_REGISTRY: OnceLock<Mutex<MediaSourceRegistry>> =
    OnceLock::new();
static ACTIVE_MEDIA_SOURCES: OnceLock<Mutex<Option<Vec<Arc<dyn MediaSourceBackend>>>>> =
    OnceLock::new();
#[allow(dead_code)]
static CAST_MEDIA_LEASES: OnceLock<Mutex<CastMediaLeaseRegistry>> = OnceLock::new();
#[allow(dead_code)]
static LAN_MEDIA_PORT: OnceLock<u16> = OnceLock::new();

struct CachedClient {
    proxy_key: Option<String>,
    client: Client,
}

#[derive(Clone)]
struct DownloadSpeedSample {
    // Attribute a completed read across its actual wall-clock interval so large range/SMB
    // chunks do not quantize the two-second rolling rate into whole MiB/s steps.
    started_at: Instant,
    finished_at: Instant,
    bytes: u64,
}

#[derive(Clone)]
struct DownloadSpeedMeter {
    generation: u64,
    generation_started_at: Instant,
    last_download_at: Option<Instant>,
    samples: VecDeque<DownloadSpeedSample>,
}

type DownloadSpeedMeterHandle = Arc<Mutex<DownloadSpeedMeter>>;

impl Default for DownloadSpeedMeter {
    fn default() -> Self {
        Self {
            generation: 0,
            generation_started_at: Instant::now(),
            last_download_at: None,
            samples: VecDeque::new(),
        }
    }
}

impl DownloadSpeedMeter {
    fn record_transfer(
        &mut self,
        generation: u64,
        bytes: usize,
        started_at: Instant,
        finished_at: Instant,
    ) {
        self.record_transfer_at(generation, bytes, started_at, finished_at);
    }

    fn record_transfer_at(
        &mut self,
        generation: u64,
        bytes: usize,
        started_at: Instant,
        finished_at: Instant,
    ) {
        if generation != self.generation || bytes == 0 {
            return;
        }
        let started_at = started_at.min(finished_at);
        let starts_new_burst = self.last_download_at.is_none()
            || self.last_download_at.is_some_and(|last_download_at| {
                started_at
                    .checked_duration_since(last_download_at)
                    .is_some_and(|idle| idle > DOWNLOAD_SPEED_STALE_AFTER)
            });
        if starts_new_burst {
            self.samples.clear();
            self.generation_started_at = started_at;
        } else {
            self.generation_started_at = self.generation_started_at.min(started_at);
        }
        self.prune_samples(finished_at);
        self.samples.push_back(DownloadSpeedSample {
            started_at,
            finished_at,
            bytes: bytes as u64,
        });
        self.last_download_at = Some(
            self.last_download_at
                .map_or(finished_at, |last_download_at| last_download_at.max(finished_at)),
        );
    }

    fn speed_bps(&self) -> f64 {
        self.speed_bps_at(Instant::now())
    }

    fn speed_bps_at(&self, now: Instant) -> f64 {
        let Some(last_download_at) = self.last_download_at else {
            return 0.0;
        };
        if now.duration_since(last_download_at) > DOWNLOAD_SPEED_STALE_AFTER {
            return 0.0;
        }
        let window_start = now
            .checked_sub(DOWNLOAD_SPEED_WINDOW)
            .unwrap_or(self.generation_started_at)
            .max(self.generation_started_at);
        let downloaded_bytes = self
            .samples
            .iter()
            .map(|sample| sample.bytes_in_window(window_start, now))
            .sum::<f64>();
        let elapsed = now
            .duration_since(window_start)
            .max(DOWNLOAD_SPEED_MIN_SAMPLE_DURATION)
            .as_secs_f64();
        downloaded_bytes / elapsed
    }

    fn begin_generation(&mut self) -> u64 {
        self.begin_generation_at(Instant::now())
    }

    fn begin_generation_at(&mut self, now: Instant) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.generation_started_at = now;
        self.last_download_at = None;
        self.samples.clear();
        self.generation
    }

    fn prune_samples(&mut self, now: Instant) {
        let cutoff = now.checked_sub(DOWNLOAD_SPEED_WINDOW);
        while self
            .samples
            .front()
            .is_some_and(|sample| {
                cutoff.is_some_and(|cutoff| sample.finished_at < cutoff)
            })
        {
            self.samples.pop_front();
        }
    }
}

impl DownloadSpeedSample {
    fn bytes_in_window(&self, window_start: Instant, window_end: Instant) -> f64 {
        if self.finished_at < window_start || self.started_at > window_end {
            return 0.0;
        }
        let transfer_duration = self.finished_at.duration_since(self.started_at);
        if transfer_duration.is_zero() {
            return self.bytes as f64;
        }
        let overlap_start = self.started_at.max(window_start);
        let overlap_end = self.finished_at.min(window_end);
        let Some(overlap) = overlap_end.checked_duration_since(overlap_start) else {
            return 0.0;
        };
        self.bytes as f64 * overlap.as_secs_f64() / transfer_duration.as_secs_f64()
    }
}

pub(crate) struct DownloadSpeedActivation {
    previous_backends: Option<Vec<Arc<dyn MediaSourceBackend>>>,
    previous_meters: Vec<(DownloadSpeedMeterHandle, DownloadSpeedMeter)>,
    committed: bool,
}

impl DownloadSpeedActivation {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for DownloadSpeedActivation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (meter, previous_meter) in self.previous_meters.drain(..) {
            if let Ok(mut current_meter) = meter.lock() {
                *current_meter = previous_meter;
            }
        }
        if let Ok(mut active_backends) = ACTIVE_MEDIA_SOURCES
            .get_or_init(|| Mutex::new(None))
            .lock()
        {
            *active_backends = self.previous_backends.take();
        }
    }
}

pub(crate) struct DownloadSpeedGeneration {
    previous_meters: Vec<(DownloadSpeedMeterHandle, DownloadSpeedMeter)>,
    committed: bool,
}

impl DownloadSpeedGeneration {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for DownloadSpeedGeneration {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (meter, previous_meter) in self.previous_meters.drain(..) {
            if let Ok(mut current_meter) = meter.lock() {
                *current_meter = previous_meter;
            }
        }
    }
}

#[derive(Clone)]
struct DownloadSpeedRecorder {
    meter: DownloadSpeedMeterHandle,
    generation: u64,
}

impl DownloadSpeedRecorder {
    fn new(meter: DownloadSpeedMeterHandle) -> Self {
        let generation = meter.lock().map(|meter| meter.generation).unwrap_or(0);
        Self { meter, generation }
    }

    fn record_transfer(&self, bytes: usize, started_at: Instant, finished_at: Instant) {
        if let Ok(mut meter) = self.meter.lock() {
            meter.record_transfer(self.generation, bytes, started_at, finished_at);
        }
    }
}

#[derive(Clone)]
struct ByteRange {
    start: u64,
    end: u64,
}

#[derive(Clone)]
struct ParallelRangePlan {
    response_start: u64,
    response_end: u64,
    total_size: u64,
    content_length: u64,
}

struct MediaSourceEntry {
    backend: Arc<dyn MediaSourceBackend>,
    last_access: Instant,
}

struct MediaSourceRegistry {
    entries: HashMap<String, MediaSourceEntry>,
    last_cleanup: Instant,
}

impl MediaSourceRegistry {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            entries: HashMap::new(),
            last_cleanup: now,
        }
    }

    fn insert(&mut self, token: String, backend: Arc<dyn MediaSourceBackend>) {
        let now = Instant::now();
        self.cleanup_if_due(now);
        self.entries.insert(
            token,
            MediaSourceEntry {
                backend,
                last_access: now,
            },
        );
        self.enforce_limit(now);
    }

    fn get(&mut self, token: &str) -> Option<Arc<dyn MediaSourceBackend>> {
        let now = Instant::now();
        self.cleanup_if_due(now);
        let entry = self.entries.get_mut(token)?;
        entry.last_access = now;
        Some(entry.backend.clone())
    }

    #[allow(dead_code)] // Used by cast-session revocation once CastingService is registered.
    fn remove(&mut self, token: &str) -> Option<Arc<dyn MediaSourceBackend>> {
        self.entries.remove(token).map(|entry| entry.backend)
    }

    fn has_origin(&self, origin: &str) -> bool {
        self.entries
            .values()
            .any(|entry| entry.backend.origin() == origin)
    }

    fn find_token_by_origin(&mut self, origin: &str) -> Option<String> {
        let now = Instant::now();
        self.cleanup_if_due(now);
        for (token, entry) in self.entries.iter_mut() {
            if entry.backend.origin() == origin {
                entry.last_access = now;
                return Some(token.clone());
            }
        }
        None
    }

    fn cleanup_if_due(&mut self, now: Instant) {
        if now.duration_since(self.last_cleanup) < MEDIA_SOURCE_CLEANUP_INTERVAL
            && self.entries.len() <= MEDIA_SOURCE_MAX_ENTRIES
        {
            return;
        }
        self.cleanup_idle(now);
    }

    fn cleanup_idle(&mut self, now: Instant) {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| now.duration_since(entry.last_access) <= MEDIA_SOURCE_IDLE_TIMEOUT);
        self.last_cleanup = now;
        let removed = before.saturating_sub(self.entries.len());
        if removed > 0 {
            debug!("media gateway: cleaned up {removed} idle backend token(s)");
        }
    }

    fn enforce_limit(&mut self, now: Instant) {
        if self.entries.len() <= MEDIA_SOURCE_MAX_ENTRIES {
            return;
        }
        let remove_count = self
            .entries
            .len()
            .saturating_sub(MEDIA_SOURCE_TARGET_ENTRIES);
        let mut oldest = self
            .entries
            .iter()
            .map(|(token, entry)| (token.clone(), entry.last_access))
            .collect::<Vec<_>>();
        oldest.sort_by_key(|(_, last_access)| *last_access);
        for (token, _) in oldest.into_iter().take(remove_count) {
            self.entries.remove(&token);
        }
        self.last_cleanup = now;
        debug!(
            "media gateway: evicted {remove_count} backend token(s) to enforce registry limit"
        );
    }
}

trait MediaSourceBackend: Send + Sync {
    fn label(&self) -> &'static str;

    fn origin(&self) -> &str;

    fn download_speed_meter(&self) -> &DownloadSpeedMeterHandle;

    fn handle<'a>(
        &'a self,
        app_handle: Option<&'a AppHandle>,
        stream: &'a mut TcpStream,
        method: &'a str,
        range: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(), String>>;
}

#[derive(Clone)]
struct CastMediaEndpoint {
    public_base_url: String,
    lease_token: String,
}

struct HttpMediaSourceBackend {
    url: String,
    download_speed_meter: DownloadSpeedMeterHandle,
    cast_endpoint: Option<CastMediaEndpoint>,
}

impl HttpMediaSourceBackend {
    fn new(url: String) -> Self {
        Self::with_download_speed_meter(url, Arc::new(Mutex::new(DownloadSpeedMeter::default())))
    }

    fn with_download_speed_meter(url: String, download_speed_meter: DownloadSpeedMeterHandle) -> Self {
        Self {
            url,
            download_speed_meter,
            cast_endpoint: None,
        }
    }

    fn for_cast(url: String, cast_endpoint: CastMediaEndpoint) -> Self {
        Self::for_cast_with_download_speed_meter(
            url,
            cast_endpoint,
            Arc::new(Mutex::new(DownloadSpeedMeter::default())),
        )
    }

    fn for_cast_with_download_speed_meter(
        url: String,
        cast_endpoint: CastMediaEndpoint,
        download_speed_meter: DownloadSpeedMeterHandle,
    ) -> Self {
        Self {
            url,
            download_speed_meter,
            cast_endpoint: Some(cast_endpoint),
        }
    }
}

impl MediaSourceBackend for HttpMediaSourceBackend {
    fn label(&self) -> &'static str {
        "http"
    }

    fn origin(&self) -> &str {
        &self.url
    }

    fn download_speed_meter(&self) -> &DownloadSpeedMeterHandle {
        &self.download_speed_meter
    }

    fn handle<'a>(
        &'a self,
        app_handle: Option<&'a AppHandle>,
        stream: &'a mut TcpStream,
        method: &'a str,
        range: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let app_handle = app_handle
                .ok_or_else(|| "HTTP media sources require an application handle".to_string())?;
            let download_speed_recorder =
                DownloadSpeedRecorder::new(self.download_speed_meter.clone());
            handle_http_stream_source(
                app_handle,
                stream,
                method,
                &self.url,
                range,
                &download_speed_recorder,
                self.cast_endpoint.as_ref(),
            )
            .await
        })
    }
}

/// Read-only backend for one already-resolved local file. It never accepts a path from an HTTP
/// request and opens a separate file handle per request, so concurrent Range reads do not share a
/// seek cursor.
struct LocalFileMediaSourceBackend {
    path: PathBuf,
    origin_label: String,
    file_size: u64,
    download_speed_meter: DownloadSpeedMeterHandle,
}

impl LocalFileMediaSourceBackend {
    fn new(path: &Path) -> Result<Self, String> {
        let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("cast local media source is not a regular file".to_string());
        }
        let origin_label = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(|name| format!("file:{name}"))
            .unwrap_or_else(|| "file:<unnamed>".to_string());
        Ok(Self {
            path,
            origin_label,
            file_size: metadata.len(),
            download_speed_meter: Arc::new(Mutex::new(DownloadSpeedMeter::default())),
        })
    }

    fn validate_file_state(&self) -> Result<(), String> {
        let metadata = std::fs::metadata(&self.path)
            .map_err(|_| "cast local media file is no longer available".to_string())?;
        if !metadata.is_file() || metadata.len() != self.file_size {
            return Err("cast local media file changed during the cast session".to_string());
        }
        Ok(())
    }

    async fn handle_local_file(
        &self,
        stream: &mut TcpStream,
        method: &str,
        range: Option<&str>,
    ) -> Result<(), String> {
        self.validate_file_state()?;
        let (status, reason, start, content_length, content_range) = match range {
            None => (200, "OK", 0, self.file_size, None),
            Some(range) => match parse_single_byte_range(range, self.file_size) {
                Some((start, end)) => (
                    206,
                    "Partial Content",
                    start,
                    end.saturating_sub(start).saturating_add(1),
                    Some(format!("bytes {start}-{end}/{}", self.file_size)),
                ),
                None => {
                    write_response(
                        stream,
                        416,
                        "Range Not Satisfiable",
                        infer_media_mime(&self.path),
                        Some(0),
                        Some(&format!("bytes */{}", self.file_size)),
                        Some("bytes"),
                    )
                    .await?;
                    return Ok(());
                }
            },
        };
        write_response(
            stream,
            status,
            reason,
            infer_media_mime(&self.path),
            Some(content_length),
            content_range.as_deref(),
            Some("bytes"),
        )
        .await?;
        if method == "HEAD" || content_length == 0 {
            return Ok(());
        }

        let path = self.path.clone();
        let mut file = tauri::async_runtime::spawn_blocking(move || -> Result<File, String> {
            let mut file = File::open(path).map_err(|error| error.to_string())?;
            file.seek(SeekFrom::Start(start)).map_err(|error| error.to_string())?;
            Ok(file)
        })
        .await
        .map_err(|error| format!("local file open task failed: {error}"))??;
        let mut remaining = content_length;
        let recorder = DownloadSpeedRecorder::new(self.download_speed_meter.clone());
        while remaining > 0 {
            let read_len = remaining.min(LOCAL_FILE_READ_CHUNK_BYTES as u64) as usize;
            let read_started_at = Instant::now();
            let (next_file, bytes) = tauri::async_runtime::spawn_blocking(move || -> Result<(File, Vec<u8>), String> {
                let mut bytes = vec![0; read_len];
                let read = file.read(&mut bytes).map_err(|error| error.to_string())?;
                bytes.truncate(read);
                Ok((file, bytes))
            })
            .await
            .map_err(|error| format!("local file read task failed: {error}"))??;
            file = next_file;
            if bytes.is_empty() {
                return Err("cast local media file was truncated during streaming".to_string());
            }
            recorder.record_transfer(bytes.len(), read_started_at, Instant::now());
            stream.write_all(&bytes).await.map_err(|error| error.to_string())?;
            remaining = remaining.saturating_sub(bytes.len() as u64);
        }
        Ok(())
    }
}

impl MediaSourceBackend for LocalFileMediaSourceBackend {
    fn label(&self) -> &'static str { "local-file" }

    fn origin(&self) -> &str { &self.origin_label }

    fn download_speed_meter(&self) -> &DownloadSpeedMeterHandle { &self.download_speed_meter }

    fn handle<'a>(
        &'a self,
        _app_handle: Option<&'a AppHandle>,
        stream: &'a mut TcpStream,
        method: &'a str,
        range: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.handle_local_file(stream, method, range).await })
    }
}

pub(crate) fn infer_media_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "ts" => "video/mp2t",
        "m3u8" => "application/vnd.apple.mpegurl",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}

struct SmbMediaSourceBackend {
    url: String,
    open_url: String,
    file: Arc<Mutex<Option<crate::network::protocols::smb::SmbPlaybackFile>>>,
    download_speed_meter: DownloadSpeedMeterHandle,
}

impl SmbMediaSourceBackend {
    fn new(url: String, open_url: String) -> Self {
        Self {
            url,
            open_url,
            file: Arc::new(Mutex::new(None)),
            download_speed_meter: Arc::new(Mutex::new(DownloadSpeedMeter::default())),
        }
    }

    async fn ensure_open(&self) -> Result<(), String> {
        {
            let guard = self.file.lock().map_err(|error| error.to_string())?;
            if guard.is_some() {
                return Ok(());
            }
        }
        let opened =
            crate::network::protocols::smb::open_playback_url(self.open_url.clone()).await?;
        let mut guard = self.file.lock().map_err(|error| error.to_string())?;
        if guard.is_none() {
            *guard = Some(opened);
        }
        Ok(())
    }

    fn clear_playback_file(&self) {
        if let Ok(mut guard) = self.file.lock() {
            *guard = None;
        }
    }

    async fn file_size(&self) -> Result<Option<u64>, String> {
        self.ensure_open().await?;
        let guard = self.file.lock().map_err(|error| error.to_string())?;
        Ok(guard.as_ref().and_then(|f| f.file_size()))
    }

    fn smb_chunk_size(
        file: &Arc<Mutex<Option<crate::network::protocols::smb::SmbPlaybackFile>>>,
    ) -> u64 {
        let negotiated = file
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref()?.max_read_size())
            .map(u64::from)
            .unwrap_or(SMB_STREAM_CHUNK_BYTES);
        let chunk_size = negotiated.min(SMB_STREAM_CHUNK_BYTES).max(64 * 1024);
        debug!("media gateway: SMB chunk size negotiated={negotiated} effective={chunk_size}");
        chunk_size
    }

    async fn read_range(
        &self,
        offset: u64,
        length: usize,
    ) -> Result<crate::network::protocols::smb::SmbReadRangeResult, String> {
        match self.read_range_once(offset, length).await {
            Ok(result) => Ok(result),
            Err(first_error) => {
                warn!(
                    "media gateway: SMB persistent read failed, reconnecting url={} offset={} length={} error={first_error}",
                    redact_url(&self.url),
                    offset,
                    length
                );
                self.clear_playback_file();
                self.read_range_once(offset, length).await
            }
        }
    }

    async fn read_range_once(
        &self,
        offset: u64,
        length: usize,
    ) -> Result<crate::network::protocols::smb::SmbReadRangeResult, String> {
        self.ensure_open().await?;
        let file = self.file.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut guard = file.lock().map_err(|error| error.to_string())?;
            let f = guard
                .as_mut()
                .ok_or_else(|| "SMB file is not open".to_string())?;
            f.read_range(offset, length)
        })
        .await
        .map_err(|error| format!("SMB read task failed: {error}"))?
    }

    async fn read_pipeline(
        &self,
        requests: &[(u64, u32)],
    ) -> Result<crate::network::protocols::smb::SmbReadRangeResult, String> {
        match self.read_pipeline_once(requests).await {
            Ok(result) => Ok(result),
            Err(first_error) => {
                warn!(
                    "media gateway: SMB pipeline read failed, reconnecting url={} error={first_error}",
                    redact_url(&self.url),
                );
                self.clear_playback_file();
                self.read_pipeline_once(requests).await
            }
        }
    }

    async fn read_pipeline_once(
        &self,
        requests: &[(u64, u32)],
    ) -> Result<crate::network::protocols::smb::SmbReadRangeResult, String> {
        self.ensure_open().await?;
        let file = self.file.clone();
        let requests = requests.to_vec();
        tauri::async_runtime::spawn_blocking(move || {
            let mut guard = file.lock().map_err(|error| error.to_string())?;
            let f = guard
                .as_mut()
                .ok_or_else(|| "SMB file is not open".to_string())?;
            f.read_pipeline(&requests)
        })
        .await
        .map_err(|error| format!("SMB pipeline task failed: {error}"))?
    }

    async fn handle_smb_stream_source(
        &self,
        stream: &mut TcpStream,
        method: &str,
        range: Option<&str>,
    ) -> Result<(), String> {
        handle_smb_stream_source(self, stream, method, &self.url, range).await
    }
}

impl MediaSourceBackend for SmbMediaSourceBackend {
    fn label(&self) -> &'static str {
        "smb"
    }

    fn origin(&self) -> &str {
        &self.url
    }

    fn download_speed_meter(&self) -> &DownloadSpeedMeterHandle {
        &self.download_speed_meter
    }

    fn handle<'a>(
        &'a self,
        _app_handle: Option<&'a AppHandle>,
        stream: &'a mut TcpStream,
        method: &'a str,
        range: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.handle_smb_stream_source(stream, method, range).await })
    }
}

enum RequestHeaderRead {
    Empty,
    Complete(Vec<u8>),
    TooLarge,
    Incomplete,
}

pub(crate) fn set_parallel_range_enabled(enabled: bool) {
    MEDIA_GATEWAY_PARALLEL_RANGE_ENABLED.store(enabled, Ordering::Release);
    info!(
        "media gateway: parallel range download {}",
        if enabled { "enabled" } else { "disabled" }
    );
}

fn parallel_range_enabled() -> bool {
    MEDIA_GATEWAY_PARALLEL_RANGE_ENABLED.load(Ordering::Acquire)
}

fn initialize_parallel_range_setting(app_handle: &AppHandle) {
    let enabled = crate::store::ui_state_store::load_setting_value(
        app_handle,
        PARALLEL_RANGE_SETTING_LABEL,
    )
    .ok()
    .flatten()
    .map(|value| !value.eq_ignore_ascii_case("off"))
    .unwrap_or(false);
    set_parallel_range_enabled(enabled);
}

pub(crate) fn register_basic_auth(playback_url: &str, username: &str, password: &str) {
    let username = username.trim();
    if username.is_empty() {
        return;
    }
    if let Ok(mut auth_map) = MEDIA_SOURCE_BASIC_AUTH
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
    {
        auth_map.insert(
            playback_url.to_string(),
            (username.to_string(), password.to_string()),
        );
    }
}

pub(crate) fn create_loopback_media_url_with_headers(
    url: &str,
    headers: &[(String, String)],
) -> Option<String> {
    if !is_http_url(url) {
        return None;
    }
    register_headers(url, headers);
    let proxied = create_loopback_media_url_for_http(url)?;
    info!("media gateway: rewrote yt-dlp stream url={}", redact_url(url));
    Some(proxied)
}

pub(crate) fn register_headers(playback_url: &str, headers: &[(String, String)]) {
    let normalized = normalize_headers(headers);
    if normalized.is_empty() {
        return;
    }
    if let Ok(mut headers_map) = MEDIA_SOURCE_HEADERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        headers_map.insert(playback_url.to_string(), normalized);
    }
}

pub(crate) fn start_loopback_listener(app_handle: AppHandle) -> Result<(), String> {
    initialize_parallel_range_setting(&app_handle);

    if LOOPBACK_MEDIA_BASE_URL.get().is_some() {
        return Ok(());
    }

    let listener = StdTcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;

    let base_url = format!("http://{addr}");
    let _ = LOOPBACK_MEDIA_BASE_URL.set(base_url);

    std::thread::Builder::new()
        .name("soia-media-gateway".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_io()
                .enable_time()
                .thread_name("soia-media-gateway-worker")
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    warn!("media gateway: failed to create async runtime: {error}");
                    return;
                }
            };
            runtime.block_on(async move {
                match TcpListener::from_std(listener) {
                    Ok(listener) => serve_loopback(listener, app_handle).await,
                    Err(error) => warn!("media gateway: failed to adopt listener: {error}"),
                }
            });
        })
        .map_err(|error| error.to_string())?;

    info!("media gateway: loopback listener on http://{addr}");
    Ok(())
}

#[allow(dead_code)]
fn start_lan_listener(app_handle: AppHandle) -> Result<u16, String> {
    if let Some(port) = LAN_MEDIA_PORT.get() {
        return Ok(*port);
    }

    let listener = StdTcpListener::bind(("0.0.0.0", 0)).map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let port = addr.port();
    std::thread::Builder::new()
        .name("soia-cast-media-gateway".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_io()
                .enable_time()
                .thread_name("soia-cast-media-gateway-worker")
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    warn!("media gateway: failed to create LAN runtime: {error}");
                    return;
                }
            };
            runtime.block_on(async move {
                match TcpListener::from_std(listener) {
                    Ok(listener) => {
                        tokio::spawn(async {
                            loop {
                                tokio::time::sleep(CAST_MEDIA_LEASE_CLEANUP_INTERVAL).await;
                                cleanup_expired_cast_media_leases();
                            }
                        });
                        serve_lan(listener, app_handle).await;
                    }
                    Err(error) => warn!("media gateway: failed to adopt LAN listener: {error}"),
                }
            });
        })
        .map_err(|error| error.to_string())?;
    let _ = LAN_MEDIA_PORT.set(port);
    info!("media gateway: LAN listener bound on port {port}");
    Ok(port)
}

pub(crate) fn create_loopback_https_media_url(url: &str) -> Option<String> {
    if !is_https_url(url) {
        return None;
    }
    let proxied = create_loopback_media_url_for_http(url)?;
    info!("media gateway: rewrote HTTPS stream url={}", redact_url(url));
    Some(proxied)
}

pub(crate) fn create_loopback_http_media_url(url: &str) -> Option<String> {
    if !is_http_url(url) {
        return None;
    }
    let proxied = create_loopback_media_url_for_http(url)?;
    info!("media gateway: rewrote HTTP stream url={}", redact_url(url));
    Some(proxied)
}

pub(crate) fn create_loopback_smb_media_url(url: &str) -> Option<String> {
    if !crate::mpv::USE_SMB_STREAM_PROXY {
        return None;
    }
    if !is_smb_url(url) {
        return None;
    }
    // Reuse an existing backend for the same origin URL if available
    if let Some(proxied) = loopback_media_url_for_existing_origin(url) {
        info!("media gateway: reused SMB backend url={}", redact_url(url));
        return Some(proxied);
    }
    let open_url = lookup_basic_auth(url)
        .and_then(|(username, password)| {
            crate::network::protocols::smb::playback_url_with_credentials(
                url,
                &username,
                &password,
            )
            .ok()
        })
        .unwrap_or_else(|| url.to_string());
    let proxied = create_loopback_media_url_for_source(Arc::new(SmbMediaSourceBackend::new(
        url.to_string(),
        open_url,
    )))?;
    info!("media gateway: rewrote SMB stream url={}", redact_url(url));
    Some(proxied)
}

/// Registers an HTTP(S) source for one receiver and returns an opaque LAN URL. This endpoint is
/// intentionally not a general-purpose URL proxy: the source is fixed before the lease exists.
#[allow(dead_code)]
pub(crate) fn create_cast_http_media_url(
    app_handle: AppHandle,
    session_id: &str,
    receiver_ip: Ipv4Addr,
    source_url: &str,
) -> Result<String, String> {
    if !is_http_url(source_url) {
        return Err("cast media gateway only accepts HTTP(S) media sources".to_string());
    }
    create_cast_media_url_for_backend(
        app_handle,
        session_id,
        receiver_ip,
        |endpoint| Arc::new(HttpMediaSourceBackend::for_cast(source_url.to_string(), endpoint)),
    )
}

/// Creates a LAN URL for one pre-validated local video file. The request path can never select a
/// different file; the only file path remains encapsulated by the backend instance.
#[allow(dead_code)]
pub(crate) fn create_cast_local_file_media_url(
    app_handle: AppHandle,
    session_id: &str,
    receiver_ip: Ipv4Addr,
    file_path: &Path,
) -> Result<String, String> {
    let backend = LocalFileMediaSourceBackend::new(file_path)?;
    create_cast_media_url_for_backend(app_handle, session_id, receiver_ip, move |_| Arc::new(backend))
}

#[allow(dead_code)]
pub(crate) fn create_cast_smb_media_url(
    app_handle: AppHandle,
    session_id: &str,
    receiver_ip: Ipv4Addr,
    source_url: &str,
) -> Result<String, String> {
    if !is_smb_url(source_url) {
        return Err("cast media gateway only accepts SMB media sources".to_string());
    }
    let open_url = lookup_basic_auth(source_url)
        .and_then(|(username, password)| {
            crate::network::protocols::smb::playback_url_with_credentials(
                source_url,
                &username,
                &password,
            )
            .ok()
        })
        .unwrap_or_else(|| source_url.to_string());
    create_cast_media_url_for_backend(
        app_handle,
        session_id,
        receiver_ip,
        move |_| Arc::new(SmbMediaSourceBackend::new(source_url.to_string(), open_url)),
    )
}

fn create_cast_media_url_for_backend<F>(
    app_handle: AppHandle,
    session_id: &str,
    receiver_ip: Ipv4Addr,
    create_backend: F,
) -> Result<String, String>
where
    F: FnOnce(CastMediaEndpoint) -> Arc<dyn MediaSourceBackend>,
{
    cleanup_expired_cast_media_leases();
    let port = match start_lan_listener(app_handle) {
        Ok(port) => port,
        Err(error) => return Err(error),
    };
    let local_ip = match crate::network::local_address::local_ipv4_for_target(receiver_ip) {
        Ok(ip) => ip,
        Err(error) => return Err(error),
    };
    let endpoint = CastMediaEndpoint {
        public_base_url: format!("http://{local_ip}:{port}"),
        lease_token: uuid::Uuid::new_v4().to_string(),
    };
    let source_id = register_media_source(create_backend(endpoint.clone()))
        .ok_or_else(|| "failed to register cast media source".to_string())?;
    let lease = CastMediaLease::new_with_token(
        endpoint.lease_token,
        session_id.to_string(),
        source_id.clone(),
        IpAddr::V4(receiver_ip),
        CAST_MEDIA_LEASE_TTL,
    );
    let media_path = lease.media_path();
    if let Err(error) = cast_media_leases()
        .lock()
        .map_err(|error| error.to_string())?
        .insert(lease)
    {
        remove_media_sources(vec![source_id]);
        return Err(error);
    }
    Ok(format!("{}{media_path}", endpoint.public_base_url))
}

/// Revokes every LAN URL and source backend belonging to a cast session.
#[allow(dead_code)]
pub(crate) fn revoke_cast_media_session(session_id: &str) {
    let revoked = cast_media_leases()
        .lock()
        .map(|mut leases| leases.revoke_session(session_id))
        .unwrap_or_default();
    release_cast_media_leases(revoked);
}

fn is_http_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
}

fn is_https_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
}

fn is_smb_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme().eq_ignore_ascii_case("smb")
}

fn media_source_registry() -> &'static Mutex<MediaSourceRegistry> {
    MEDIA_SOURCE_REGISTRY.get_or_init(|| Mutex::new(MediaSourceRegistry::new()))
}

fn remove_media_sources(source_ids: impl IntoIterator<Item = String>) {
    let removed_origins = {
        let Ok(mut sources) = media_source_registry().lock() else {
            return;
        };
        let removed_origins = source_ids
            .into_iter()
            .filter_map(|source_id| sources.remove(&source_id))
            .map(|source| source.origin().to_string())
            .collect::<Vec<_>>();
        removed_origins
            .into_iter()
            .filter(|origin| !sources.has_origin(origin))
            .collect::<Vec<_>>()
    };
    if removed_origins.is_empty() {
        return;
    }
    if let Ok(mut auth) = MEDIA_SOURCE_BASIC_AUTH
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        for origin in &removed_origins {
            auth.remove(origin);
        }
    }
    if let Ok(mut headers) = MEDIA_SOURCE_HEADERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        for origin in &removed_origins {
            headers.remove(origin);
        }
    }
}

fn release_cast_media_leases(leases: impl IntoIterator<Item = CastMediaLease>) {
    let source_ids = leases
        .into_iter()
        .flat_map(|lease| lease.source_ids().to_vec())
        .collect::<Vec<_>>();
    remove_media_sources(source_ids);
}

fn cleanup_expired_cast_media_leases() {
    let expired = cast_media_leases()
        .lock()
        .map(|mut leases| leases.purge_expired(Instant::now()))
        .unwrap_or_default();
    release_cast_media_leases(expired);
}

#[allow(dead_code)]
fn cast_media_leases() -> &'static Mutex<CastMediaLeaseRegistry> {
    CAST_MEDIA_LEASES.get_or_init(|| Mutex::new(CastMediaLeaseRegistry::new()))
}

pub(crate) fn download_speed_bps() -> f64 {
    ACTIVE_MEDIA_SOURCES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|backends| backends.as_ref().cloned())
        .map(|backends| {
            backends
                .iter()
                .filter_map(|backend| {
                    backend
                        .download_speed_meter()
                        .lock()
                        .ok()
                        .map(|meter| meter.speed_bps())
                })
                .sum()
        })
        .unwrap_or(0.0)
}

pub(crate) fn begin_download_speed_activation(url: &str) -> DownloadSpeedActivation {
    let backends = media_sources_for_playback_url(url);
    let previous_meters = backends
        .iter()
        .filter_map(|backend| {
            let meter = backend.download_speed_meter().clone();
            let previous_meter = meter.lock().ok().map(|mut current_meter| {
                let previous_meter = current_meter.clone();
                current_meter.begin_generation();
                previous_meter
            });
            previous_meter.map(|previous_meter| (meter, previous_meter))
        })
        .collect();
    let previous_backends = ACTIVE_MEDIA_SOURCES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut active_backends| {
            std::mem::replace(&mut *active_backends, (!backends.is_empty()).then_some(backends))
        });
    DownloadSpeedActivation {
        previous_backends,
        previous_meters,
        committed: false,
    }
}

pub(crate) fn begin_download_speed_generation() -> Option<DownloadSpeedGeneration> {
    let backends = ACTIVE_MEDIA_SOURCES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|backends| backends.as_ref().cloned())?;
    let previous_meters = backends
        .iter()
        .filter_map(|backend| {
            let meter = backend.download_speed_meter().clone();
            let previous_meter = meter.lock().ok().map(|mut current_meter| {
                let previous_meter = current_meter.clone();
                current_meter.begin_generation();
                previous_meter
            });
            previous_meter.map(|previous_meter| (meter, previous_meter))
        })
        .collect::<Vec<_>>();
    if previous_meters.is_empty() {
        return None;
    }
    Some(DownloadSpeedGeneration {
        previous_meters,
        committed: false,
    })
}

pub(crate) fn is_loopback_media_url(url: &str) -> bool {
    let Some(base_url) = LOOPBACK_MEDIA_BASE_URL.get() else {
        return false;
    };
    let (Ok(base), Ok(candidate)) = (Url::parse(base_url), Url::parse(url)) else {
        return false;
    };
    base.scheme() == candidate.scheme()
        && base.host_str() == candidate.host_str()
        && base.port_or_known_default() == candidate.port_or_known_default()
        && candidate.path().starts_with("/stream/")
}

fn create_loopback_media_url_for_http(raw: &str) -> Option<String> {
    create_loopback_media_url_for_source(Arc::new(HttpMediaSourceBackend::new(raw.to_string())))
}

fn create_loopback_media_url_for_source(backend: Arc<dyn MediaSourceBackend>) -> Option<String> {
    let base = LOOPBACK_MEDIA_BASE_URL.get()?;
    let token = register_media_source(backend)?;
    Some(format!("{base}/stream/{token}"))
}

fn register_media_source(backend: Arc<dyn MediaSourceBackend>) -> Option<String> {
    let token = uuid::Uuid::now_v7().to_string();
    media_source_registry().lock().ok()?.insert(token.clone(), backend);
    Some(token)
}

fn loopback_media_url_for_existing_origin(origin: &str) -> Option<String> {
    let base = LOOPBACK_MEDIA_BASE_URL.get()?;
    let token = media_source_registry().lock().ok()?.find_token_by_origin(origin)?;
    Some(format!("{base}/stream/{token}"))
}

fn media_sources_for_playback_url(url: &str) -> Vec<Arc<dyn MediaSourceBackend>> {
    let urls = if url.starts_with("edl://") {
        edl_stream_urls(url)
    } else {
        vec![url]
    };
    urls.into_iter()
        .filter_map(media_source_for_loopback_url)
        .fold(Vec::new(), |mut backends, backend| {
            if !backends.iter().any(|existing| Arc::ptr_eq(existing, &backend)) {
                backends.push(backend);
            }
            backends
        })
}

fn media_source_for_loopback_url(url: &str) -> Option<Arc<dyn MediaSourceBackend>> {
    let target = Url::parse(url).ok()?.path().to_string();
    let token = target.strip_prefix("/stream/")?.trim();
    (!token.is_empty()).then(|| media_source_registry().lock().ok()?.get(token))?
}

fn edl_stream_urls(value: &str) -> Vec<&str> {
    let mut urls = Vec::new();
    let mut rest = value.strip_prefix("edl://").unwrap_or(value);
    while let Some(marker_start) = rest.find('%') {
        rest = &rest[marker_start + 1..];
        let Some(length_end) = rest.find('%') else {
            break;
        };
        let Ok(length) = rest[..length_end].parse::<usize>() else {
            continue;
        };
        rest = &rest[length_end + 1..];
        if rest.len() < length {
            break;
        }
        let (url, remaining) = rest.split_at(length);
        urls.push(url);
        rest = remaining;
    }
    urls
}

fn lookup_media_source(target: &str) -> Option<Arc<dyn MediaSourceBackend>> {
    if let Some(remote_url) = parse_remote_url(target) {
        return Some(Arc::new(HttpMediaSourceBackend::new(remote_url)));
    }
    let path = target.split_once('?').map(|(path, _)| path).unwrap_or(target);
    let token = path.strip_prefix("/stream/")?.trim();
    if token.is_empty() {
        return None;
    }
    media_source_registry().lock().ok()?.get(token)
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

fn is_client_disconnect_error(error: &str) -> bool {
    error.contains("Broken pipe")
        || error.contains("Connection reset by peer")
        || error.contains("connection reset by peer")
}

async fn serve_loopback(listener: TcpListener, app_handle: AppHandle) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let app_handle = app_handle.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, &app_handle).await {
                        if is_client_disconnect_error(&error) {
                            debug!("media gateway: client disconnected: {error}");
                        } else {
                            warn!("media gateway: request failed: {error}");
                        }
                    }
                });
            }
            Err(error) => warn!("media gateway: accept failed: {error}"),
        }
    }
}

#[allow(dead_code)]
async fn serve_lan(listener: TcpListener, app_handle: AppHandle) {
    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let app_handle = app_handle.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_lan_connection(stream, peer_addr.ip(), &app_handle).await {
                        if is_client_disconnect_error(&error) {
                            debug!("media gateway: LAN client disconnected: {error}");
                        } else {
                            warn!("media gateway: LAN request failed: {error}");
                        }
                    }
                });
            }
            Err(error) => warn!("media gateway: LAN accept failed: {error}"),
        }
    }
}

#[allow(dead_code)]
async fn handle_lan_connection(
    stream: TcpStream,
    client_ip: IpAddr,
    app_handle: &AppHandle,
) -> Result<(), String> {
    handle_lan_connection_with_context(stream, client_ip, Some(app_handle)).await
}

async fn handle_lan_connection_with_context(
    mut stream: TcpStream,
    client_ip: IpAddr,
    app_handle: Option<&AppHandle>,
) -> Result<(), String> {
    cleanup_expired_cast_media_leases();
    let request_bytes = match read_request_header(&mut stream).await? {
        RequestHeaderRead::Empty => return Ok(()),
        RequestHeaderRead::Complete(bytes) => bytes,
        RequestHeaderRead::TooLarge => {
            write_status(&mut stream, 431, "Request Header Fields Too Large", b"request header too large").await?;
            return Ok(());
        }
        RequestHeaderRead::Incomplete => {
            write_status(&mut stream, 400, "Bad Request", b"incomplete request header").await?;
            return Ok(());
        }
    };
    let request = String::from_utf8_lossy(&request_bytes);
    let (method, target, range) = parse_request(&request)?;
    if method != "GET" && method != "HEAD" {
        write_status(&mut stream, 405, "Method Not Allowed", b"method not allowed").await?;
        return Ok(());
    }
    let Some(route) = parse_cast_media_route(&target) else {
        write_status(&mut stream, 404, "Not Found", b"cast media not found").await?;
        return Ok(());
    };
    let source_id = cast_media_leases()
        .lock()
        .ok()
        .and_then(|mut leases| {
            let lease = leases.authorize(route.lease_token, client_ip)?;
            match route.source_id {
                Some(source_id) if lease.contains_source(source_id) => Some(source_id.to_string()),
                Some(_) => None,
                None => Some(lease.source_id.clone()),
            }
        });
    let Some(source_id) = source_id else {
        write_status(&mut stream, 404, "Not Found", b"cast media not found").await?;
        return Ok(());
    };
    let Some(backend) = media_source_registry().lock().ok().and_then(|mut backends| backends.get(&source_id)) else {
        write_status(&mut stream, 410, "Gone", b"cast media unavailable").await?;
        return Ok(());
    };
    let token_prefix = route.lease_token.chars().take(8).collect::<String>();
    debug!("media gateway: dispatch LAN backend={} token_prefix={token_prefix}", backend.label());
    backend
        .handle(app_handle, &mut stream, &method, range.as_deref())
        .await
}

#[allow(dead_code)]
fn parse_cast_media_token(target: &str) -> Option<&str> {
    match parse_cast_media_route(target)? {
        CastMediaRoute {
            lease_token,
            source_id: None,
        } => Some(lease_token),
        CastMediaRoute { source_id: Some(_), .. } => None,
    }
}

struct CastMediaRoute<'a> {
    lease_token: &'a str,
    source_id: Option<&'a str>,
}

fn parse_cast_media_route(target: &str) -> Option<CastMediaRoute<'_>> {
    if target.contains('?') {
        return None;
    }
    let value = target.strip_prefix("/cast/")?;
    if let Some(token) = value.strip_suffix("/media") {
        return (!token.is_empty() && !token.contains('/')).then_some(CastMediaRoute {
            lease_token: token,
            source_id: None,
        });
    }
    let (token, source_id) = value.split_once("/resource/")?;
    (!token.is_empty() && !source_id.is_empty() && !token.contains('/') && !source_id.contains('/'))
        .then_some(CastMediaRoute {
            lease_token: token,
            source_id: Some(source_id),
        })
}

async fn handle_connection(mut stream: TcpStream, app_handle: &AppHandle) -> Result<(), String> {
    let request_bytes = match read_request_header(&mut stream).await? {
        RequestHeaderRead::Empty => return Ok(()),
        RequestHeaderRead::Complete(bytes) => bytes,
        RequestHeaderRead::TooLarge => {
            write_status(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                b"request header too large",
            )
            .await?;
            return Ok(());
        }
        RequestHeaderRead::Incomplete => {
            write_status(&mut stream, 400, "Bad Request", b"incomplete request header").await?;
            return Ok(());
        }
    };

    let request = String::from_utf8_lossy(&request_bytes);
    let (method, target, range) = parse_request(&request)?;
    if method != "GET" && method != "HEAD" {
        write_status(&mut stream, 405, "Method Not Allowed", b"method not allowed").await?;
        return Ok(());
    }

    let Some(backend) = lookup_media_source(&target) else {
        write_status(&mut stream, 400, "Bad Request", b"missing stream source").await?;
        return Ok(());
    };

    debug!(
        "media gateway: dispatch backend={} origin={}",
        backend.label(),
        redact_url(backend.origin())
    );
    backend
        .handle(Some(app_handle), &mut stream, &method, range.as_deref())
        .await
}

async fn handle_http_stream_source(
    app_handle: &AppHandle,
    stream: &mut TcpStream,
    method: &str,
    remote_url: &str,
    range: Option<&str>,
    download_speed_recorder: &DownloadSpeedRecorder,
    cast_endpoint: Option<&CastMediaEndpoint>,
) -> Result<(), String> {
    debug!("media gateway: fetch {}", redact_url(remote_url));

    let response = match fetch_remote(app_handle, remote_url, range).await {
        Ok(response) => response,
        Err(error) => {
            warn!(
                "media gateway: upstream fetch failed url={} error={error}",
                redact_url(remote_url)
            );
            write_status(stream, 502, "Bad Gateway", error.as_bytes()).await?;
            return Ok(());
        }
    };
    let status = response.status();
    if !status.is_success() {
        let code = status.as_u16();
        let reason = status.canonical_reason().unwrap_or("Upstream Error").to_string();
        let body = response.bytes().await.map_err(|error| error.to_string())?;
        write_status(stream, code, &reason, &body).await?;
        return Ok(());
    }

    if should_rewrite_playlist(remote_url, &response) {
        let content_type = content_type(&response);
        let reason = status.canonical_reason().unwrap_or("OK").to_string();
        let read_started_at = Instant::now();
        let bytes = response.bytes().await.map_err(|error| error.to_string())?;
        download_speed_recorder.record_transfer(
            bytes.len(),
            read_started_at,
            Instant::now(),
        );
        let text = String::from_utf8_lossy(&bytes);
        let inherited_headers = lookup_headers(remote_url);
        let body = match rewrite_playlist_for_endpoint(
            remote_url,
            &text,
            inherited_headers.as_deref(),
            &download_speed_recorder.meter,
            cast_endpoint,
        ) {
            Ok(body) => body.into_bytes(),
            Err(error) => {
                warn!("media gateway: failed to register HLS child resource: {error}");
                write_status(stream, 502, "Bad Gateway", b"cast HLS resource unavailable").await?;
                return Ok(());
            }
        };
        write_response(
            stream,
            status.as_u16(),
            &reason,
            &content_type,
            Some(body.len() as u64),
            None,
            None,
        )
        .await?;
        if method != "HEAD" {
            stream
                .write_all(&body)
                .await
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    stream_response(
        app_handle,
        stream,
        method,
        remote_url,
        range,
        response,
        download_speed_recorder,
    )
    .await
}

async fn handle_smb_stream_source(
    backend: &SmbMediaSourceBackend,
    stream: &mut TcpStream,
    method: &str,
    remote_url: &str,
    range: Option<&str>,
) -> Result<(), String> {
    let download_speed_recorder =
        DownloadSpeedRecorder::new(backend.download_speed_meter.clone());
    debug!("media gateway: fetch {}", redact_url(remote_url));
    let total_size = match backend.file_size().await {
        Ok(Some(size)) => size,
        Ok(None) => {
            write_status(stream, 502, "Bad Gateway", b"SMB file size unavailable").await?;
            return Ok(());
        }
        Err(error) => {
            warn!(
                "media gateway: SMB metadata failed url={} error={error}",
                redact_url(remote_url)
            );
            return Err(error);
        }
    };

    let (status, response_start, response_end, content_range) = if let Some(range) = range {
        let parsed_range = parse_open_ended_range(Some(range))
            .and_then(|start| {
                (start < total_size).then(|| {
                    let end = total_size.saturating_sub(1);
                    (start, end)
                })
            })
            .or_else(|| parse_single_byte_range(range, total_size));
        let Some((start, end)) = parsed_range else {
            write_response(
                stream,
                StatusCode::RANGE_NOT_SATISFIABLE.as_u16(),
                StatusCode::RANGE_NOT_SATISFIABLE
                    .canonical_reason()
                    .unwrap_or("Range Not Satisfiable"),
                "text/plain; charset=utf-8",
                Some(0),
                Some(&format!("bytes */{total_size}")),
                Some("bytes"),
            )
            .await?;
            return Ok(());
        };
        (
            StatusCode::PARTIAL_CONTENT,
            start,
            end,
            Some(format!("bytes {start}-{end}/{total_size}")),
        )
    } else {
        let end = total_size.saturating_sub(1);
        (StatusCode::OK, 0, end, None)
    };

    let content_length = if total_size == 0 {
        0
    } else {
        response_end.saturating_sub(response_start).saturating_add(1)
    };
    write_response(
        stream,
        status.as_u16(),
        status.canonical_reason().unwrap_or("OK"),
        "application/octet-stream",
        Some(content_length),
        content_range.as_deref(),
        Some("bytes"),
    )
    .await?;

    if method == "HEAD" || content_length == 0 {
        return Ok(());
    }

    let chunk_size = {
        backend.ensure_open().await?;
        SmbMediaSourceBackend::smb_chunk_size(&backend.file)
    };
    let mut next = response_start;
    while next <= response_end {
        // Build a batch of pipeline requests (up to SMB_PIPELINE_DEPTH chunks)
        let mut requests: Vec<(u64, u32)> = Vec::with_capacity(SMB_PIPELINE_DEPTH);
        let mut batch_next = next;
        for _ in 0..SMB_PIPELINE_DEPTH {
            if batch_next > response_end {
                break;
            }
            let length = response_end
                .saturating_sub(batch_next)
                .saturating_add(1)
                .min(chunk_size) as u32;
            requests.push((batch_next, length));
            batch_next = batch_next.saturating_add(length as u64);
        }

        if requests.len() <= 1 {
            // Single chunk: use the simpler read_range path
            let length = requests.first().map(|r| r.1 as usize).unwrap_or(0);
            let read_started_at = Instant::now();
            let chunk = match backend.read_range(next, length).await {
                Ok(chunk) => chunk,
                Err(error) => {
                    warn!(
                        "media gateway: SMB read failed url={} offset={} length={} error={error}",
                        redact_url(remote_url),
                        next,
                        length
                    );
                    return Err(error);
                }
            };
            if chunk.data.is_empty() {
                break;
            }
            download_speed_recorder.record_transfer(
                chunk.data.len(),
                read_started_at,
                Instant::now(),
            );
            stream
                .write_all(&chunk.data)
                .await
                .map_err(|error| error.to_string())?;
            next = next.saturating_add(chunk.data.len() as u64);
        } else {
            // Multiple chunks: use pipeline read for better throughput
            let read_started_at = Instant::now();
            let batch = match backend.read_pipeline(&requests).await {
                Ok(batch) => batch,
                Err(error) => {
                    warn!(
                        "media gateway: SMB pipeline read failed url={} offset={} count={} error={error}",
                        redact_url(remote_url),
                        next,
                        requests.len()
                    );
                    return Err(error);
                }
            };
            if batch.data.is_empty() {
                break;
            }
            download_speed_recorder.record_transfer(
                batch.data.len(),
                read_started_at,
                Instant::now(),
            );
            stream
                .write_all(&batch.data)
                .await
                .map_err(|error| error.to_string())?;
            next = next.saturating_add(batch.data.len() as u64);
        }
    }

    Ok(())
}

async fn read_request_header(stream: &mut TcpStream) -> Result<RequestHeaderRead, String> {
    let mut bytes = Vec::with_capacity(16 * 1024);
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return if bytes.is_empty() {
                Ok(RequestHeaderRead::Empty)
            } else {
                Ok(RequestHeaderRead::Incomplete)
            };
        }
        bytes.extend_from_slice(&buffer[..read]);
        if request_header_end(&bytes).is_some() {
            return Ok(RequestHeaderRead::Complete(bytes));
        }
        if bytes.len() > MAX_REQUEST_HEADER_BYTES {
            return Ok(RequestHeaderRead::TooLarge);
        }
    }
}

fn request_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            bytes.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
}

fn parse_request(request: &str) -> Result<(String, String, Option<String>), String> {
    let mut lines = request.lines();
    let request_line = lines.next().ok_or_else(|| "missing request line".to_string())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let target = request_parts.next().unwrap_or_default().to_string();
    let range = lines.find_map(|line| {
        line.split_once(':').and_then(|(name, value)| {
            name.eq_ignore_ascii_case("range")
                .then(|| value.trim().to_string())
        })
    });
    Ok((method, target, range))
}

fn parse_remote_url(target: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        if name == "url" {
            Some(percent_decode_str(value).decode_utf8_lossy().to_string())
        } else {
            None
        }
    })
}

async fn fetch_remote(
    app_handle: &AppHandle,
    remote_url: &str,
    range: Option<&str>,
) -> Result<Response, String> {
    let mut last_error = String::new();
    for attempt in 0..=FETCH_REMOTE_MAX_RETRIES {
        if attempt > 0 {
            debug!(
                "media gateway: retrying fetch attempt={} url={}",
                attempt,
                redact_url(remote_url)
            );
            tokio::time::sleep(FETCH_REMOTE_RETRY_DELAY).await;
        }
        let client = build_client(app_handle)?;
        let mut request = client
            .get(remote_url)
            .header(ACCEPT_ENCODING, "identity");
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        request = apply_basic_auth(request, remote_url);
        request = apply_headers(request, remote_url);
        match request.send().await {
            Ok(response) => return Ok(response),
            Err(error) => {
                last_error = error.to_string();
                // Only retry on connection-level errors, not on HTTP-level errors.
                if !error.is_connect() && !error.is_request() {
                    break;
                }
            }
        }
    }
    Err(last_error)
}

fn build_client(app_handle: &AppHandle) -> Result<Client, String> {
    let proxy_key = crate::network::proxy::current_proxy_key(app_handle)?;
    let client_cache = MEDIA_GATEWAY_CLIENT.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = client_cache.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.proxy_key == proxy_key {
                return Ok(cached.client.clone());
            }
        }
    }

    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_zstd()
        .no_deflate();
    let client = configure_client_builder_with_proxy_key(builder, proxy_key.as_deref())?
        .build()
        .map_err(|error| error.to_string())?;

    if let Ok(mut guard) = client_cache.lock() {
        *guard = Some(CachedClient {
            proxy_key,
            client: client.clone(),
        });
    }
    Ok(client)
}

fn configure_client_builder_with_proxy_key(
    builder: reqwest::ClientBuilder,
    proxy_key: Option<&str>,
) -> Result<reqwest::ClientBuilder, String> {
    let Some(proxy_url) = proxy_key else {
        return Ok(builder);
    };
    let proxy = reqwest::Proxy::all(proxy_url).map_err(|error| error.to_string())?;
    Ok(builder.proxy(proxy))
}

fn apply_basic_auth(request: RequestBuilder, remote_url: &str) -> RequestBuilder {
    match lookup_basic_auth(remote_url) {
        Some((username, password)) => request.basic_auth(username, Some(password)),
        None => request,
    }
}

fn lookup_basic_auth(url: &str) -> Option<BasicAuth> {
    MEDIA_SOURCE_BASIC_AUTH
        .get()
        .and_then(|auth_map| auth_map.lock().ok())
        .and_then(|auth_map| auth_map.get(url).cloned())
}

fn normalize_headers(headers: &[(String, String)]) -> ProxyHeaders {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() || !should_forward_registered_header(name) {
                return None;
            }
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

fn should_forward_registered_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "user-agent"
            | "referer"
            | "cookie"
            | "origin"
            | "accept"
            | "accept-language"
            | "sec-fetch-mode"
            | "sec-fetch-site"
            | "sec-fetch-dest"
    )
}

fn apply_headers(mut request: RequestBuilder, remote_url: &str) -> RequestBuilder {
    let headers = lookup_headers(remote_url);
    let has_registered_ua = headers
        .as_ref()
        .map(|h| h.iter().any(|(n, _)| n.eq_ignore_ascii_case("user-agent")))
        .unwrap_or(false);
    if !has_registered_ua {
        request = request.header(USER_AGENT, HTTP_USER_AGENT);
    }
    let Some(headers) = headers else {
        return request;
    };
    for (name, value) in headers {
        let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(header_value) = HeaderValue::from_str(&value) else {
            continue;
        };
        request = request.header(header_name, header_value);
    }
    request
}

fn lookup_headers(url: &str) -> Option<ProxyHeaders> {
    MEDIA_SOURCE_HEADERS
        .get()
        .and_then(|headers_map| headers_map.lock().ok())
        .and_then(|headers_map| headers_map.get(url).cloned())
}

fn should_rewrite_playlist(remote_url: &str, response: &Response) -> bool {
    remote_url.to_ascii_lowercase().contains(".m3u8")
        || content_type(response).to_ascii_lowercase().contains("mpegurl")
}

fn content_type(response: &Response) -> String {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string()
}

#[cfg(test)]
fn rewrite_playlist(
    base_url: &str,
    text: &str,
    inherited_headers: Option<&[(String, String)]>,
    download_speed_meter: &DownloadSpeedMeterHandle,
) -> Result<String, String> {
    rewrite_playlist_for_endpoint(base_url, text, inherited_headers, download_speed_meter, None)
}

fn rewrite_playlist_for_endpoint(
    base_url: &str,
    text: &str,
    inherited_headers: Option<&[(String, String)]>,
    download_speed_meter: &DownloadSpeedMeterHandle,
    cast_endpoint: Option<&CastMediaEndpoint>,
) -> Result<String, String> {
    let base = Url::parse(base_url).ok();
    text.lines()
        .map(|line| {
            rewrite_playlist_line(
                base.as_ref(),
                line,
                inherited_headers,
                download_speed_meter,
                cast_endpoint,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join("\n"))
}

fn rewrite_playlist_line(
    base: Option<&Url>,
    line: &str,
    inherited_headers: Option<&[(String, String)]>,
    download_speed_meter: &DownloadSpeedMeterHandle,
    cast_endpoint: Option<&CastMediaEndpoint>,
) -> Result<String, String> {
    if line.trim().is_empty() {
        return Ok(line.to_string());
    }
    if line.starts_with('#') {
        return rewrite_uri_attributes(base, line, inherited_headers, download_speed_meter, cast_endpoint);
    }
    Ok(rewrite_playlist_url(base, line, inherited_headers, download_speed_meter, cast_endpoint)?
        .unwrap_or_else(|| line.to_string()))
}

fn rewrite_uri_attributes(
    base: Option<&Url>,
    line: &str,
    inherited_headers: Option<&[(String, String)]>,
    download_speed_meter: &DownloadSpeedMeterHandle,
    cast_endpoint: Option<&CastMediaEndpoint>,
) -> Result<String, String> {
    let mut rewritten = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(index) = rest.find("URI=\"") {
        let (before, after_prefix) = rest.split_at(index);
        rewritten.push_str(before);
        rewritten.push_str("URI=\"");
        let uri_start = &after_prefix[5..];
        let Some(end) = uri_start.find('"') else {
            rewritten.push_str(uri_start);
            return Ok(rewritten);
        };
        let uri = &uri_start[..end];
        rewritten.push_str(
            &rewrite_playlist_url(base, uri, inherited_headers, download_speed_meter, cast_endpoint)?
                .unwrap_or_else(|| uri.to_string()),
        );
        rest = &uri_start[end..];
    }
    rewritten.push_str(rest);
    Ok(rewritten)
}

fn rewrite_playlist_url(
    base: Option<&Url>,
    value: &str,
    inherited_headers: Option<&[(String, String)]>,
    download_speed_meter: &DownloadSpeedMeterHandle,
    cast_endpoint: Option<&CastMediaEndpoint>,
) -> Result<Option<String>, String> {
    let Some(resolved) = resolve_playlist_url(base, value) else {
        return Ok(None);
    };
    if let Some(headers) = inherited_headers {
        register_headers(resolved.as_str(), headers);
    }
    if !matches!(resolved.scheme(), "http" | "https") {
        return Ok(None);
    }
    if let Some(cast_endpoint) = cast_endpoint {
        if let Some((username, password)) = lookup_basic_auth(base.map(Url::as_str).unwrap_or_default()) {
            register_basic_auth(resolved.as_str(), &username, &password);
        }
        let origin = resolved.to_string();
        if let Some(source_id) = cast_media_leases()
            .lock()
            .ok()
            .and_then(|leases| leases.resource_source(&cast_endpoint.lease_token, &origin))
        {
            return Ok(Some(format!(
                "{}/cast/{}/resource/{source_id}",
                cast_endpoint.public_base_url, cast_endpoint.lease_token,
            )));
        }
        let source_id = register_media_source(Arc::new(
            HttpMediaSourceBackend::for_cast_with_download_speed_meter(
                resolved.to_string(),
                cast_endpoint.clone(),
                download_speed_meter.clone(),
            ),
        ))
        .ok_or_else(|| "failed to register cast HLS resource".to_string())?;
        let registration = cast_media_leases()
            .lock()
            .ok()
            .map(|mut leases| leases.register_resource(
                &cast_endpoint.lease_token,
                origin,
                source_id.clone(),
            ))
            .unwrap_or(ResourceRegistration::LeaseUnavailable);
        match registration {
            ResourceRegistration::Registered => {}
            ResourceRegistration::Existing(existing_source_id) => {
                remove_media_sources(vec![source_id]);
                return Ok(Some(format!(
                    "{}/cast/{}/resource/{existing_source_id}",
                    cast_endpoint.public_base_url, cast_endpoint.lease_token,
                )));
            }
            ResourceRegistration::LeaseUnavailable => {
                remove_media_sources(vec![source_id]);
                return Err("cast media lease is no longer available".to_string());
            }
            ResourceRegistration::AtCapacity => {
                remove_media_sources(vec![source_id]);
                return Err("cast HLS resource limit reached".to_string());
            }
        }
        return Ok(Some(format!(
            "{}/cast/{}/resource/{source_id}",
            cast_endpoint.public_base_url, cast_endpoint.lease_token,
        )));
    }
    Ok(create_loopback_media_url_for_source(Arc::new(
        HttpMediaSourceBackend::with_download_speed_meter(
            resolved.to_string(),
            download_speed_meter.clone(),
        ),
    )))
}

fn resolve_playlist_url(base: Option<&Url>, value: &str) -> Option<Url> {
    if let Ok(url) = Url::parse(value) {
        Some(url)
    } else {
        base?.join(value).ok()
    }
}

fn parse_open_ended_range(value: Option<&str>) -> Option<u64> {
    let value = value?;
    let range = value.trim();
    let bytes = range.strip_prefix("bytes=")?;
    let (start, end) = bytes.split_once('-')?;
    if !end.trim().is_empty() {
        return None;
    }
    start.trim().parse::<u64>().ok()
}

fn parse_single_byte_range(value: &str, total_size: u64) -> Option<(u64, u64)> {
    let range = value.trim();
    let bytes = range.strip_prefix("bytes=")?;
    if bytes.contains(',') {
        return None;
    }
    let (start, end) = bytes.split_once('-')?;
    let start = start.trim();
    let end = end.trim();
    if total_size == 0 {
        return None;
    }
    if start.is_empty() {
        let suffix_len = end.parse::<u64>().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let range_start = total_size.saturating_sub(suffix_len);
        return Some((range_start, total_size - 1));
    }

    let range_start = start.parse::<u64>().ok()?;
    if range_start >= total_size {
        return None;
    }
    let range_end = if end.is_empty() {
        total_size - 1
    } else {
        end.parse::<u64>().ok()?.min(total_size - 1)
    };
    if range_end < range_start {
        return None;
    }
    Some((range_start, range_end))
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim();
    let range = value.strip_prefix("bytes ")?;
    let (range, total) = range.split_once('/')?;
    if total == "*" {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    Some((
        start.trim().parse::<u64>().ok()?,
        end.trim().parse::<u64>().ok()?,
        total.trim().parse::<u64>().ok()?,
    ))
}

fn is_parallel_range_excluded_url(remote_url: &str) -> bool {
    let Ok(url) = Url::parse(remote_url) else {
        return true;
    };
    let path = url.path().to_ascii_lowercase();
    path.ends_with(".m3u8")
}

fn parallel_range_plan(
    remote_url: &str,
    request_range: Option<&str>,
    status: StatusCode,
    content_length: Option<u64>,
    content_range: Option<&str>,
    accept_ranges: &str,
) -> Option<ParallelRangePlan> {
    if !accept_ranges
        .split(',')
        .any(|value| value.trim().eq_ignore_ascii_case("bytes"))
    {
        return None;
    }
    if is_parallel_range_excluded_url(remote_url) {
        return None;
    }

    let plan = if status == StatusCode::OK && request_range.is_none() {
        let content_length = content_length?;
        ParallelRangePlan {
            response_start: 0,
            response_end: content_length.checked_sub(1)?,
            total_size: content_length,
            content_length,
        }
    } else if status == StatusCode::PARTIAL_CONTENT {
        let requested_start = parse_open_ended_range(request_range)?;
        let (response_start, response_end, total_size) = parse_content_range(content_range?)?;
        if response_start != requested_start {
            return None;
        }
        ParallelRangePlan {
            response_start,
            response_end,
            total_size,
            content_length: response_end.checked_sub(response_start)?.saturating_add(1),
        }
    } else {
        return None;
    };

    if plan.content_length < PARALLEL_RANGE_MIN_BYTES {
        return None;
    }
    Some(plan)
}

fn split_byte_ranges(start: u64, end: u64) -> Vec<ByteRange> {
    let mut ranges = Vec::new();
    let mut next = start;
    while next <= end {
        let chunk_end = next.saturating_add(PARALLEL_RANGE_CHUNK_BYTES - 1).min(end);
        ranges.push(ByteRange {
            start: next,
            end: chunk_end,
        });
        if chunk_end == u64::MAX {
            break;
        }
        next = chunk_end + 1;
    }
    ranges
}

async fn fetch_range_bytes(
    app_handle: &AppHandle,
    remote_url: &str,
    range: ByteRange,
    download_speed_recorder: &DownloadSpeedRecorder,
) -> Result<(u64, Vec<u8>), String> {
    let client = build_client(app_handle)?;
    let mut request = client
        .get(remote_url)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={}-{}", range.start, range.end));
    request = apply_basic_auth(request, remote_url);
    request = apply_headers(request, remote_url);
    let mut response = request.send().await.map_err(|error| error.to_string())?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "parallel range request failed: status={} range={}-{}",
            response.status(),
            range.start,
            range.end
        ));
    }
    let expected_len = range.end.saturating_sub(range.start).saturating_add(1) as usize;
    let mut bytes = Vec::with_capacity(expected_len);
    loop {
        let read_started_at = Instant::now();
        let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? else {
            break;
        };
        download_speed_recorder.record_transfer(
            chunk.len(),
            read_started_at,
            Instant::now(),
        );
        bytes.extend_from_slice(&chunk);
        if bytes.len() > expected_len {
            break;
        }
    }
    if bytes.len() != expected_len {
        return Err(format!(
            "parallel range length mismatch: expected={} actual={} range={}-{}",
            expected_len,
            bytes.len(),
            range.start,
            range.end
        ));
    }
    Ok((range.start, bytes))
}

async fn stream_parallel_range_response(
    app_handle: &AppHandle,
    stream: &mut TcpStream,
    remote_url: &str,
    plan: ParallelRangePlan,
    first_chunk: Vec<u8>,
    download_speed_recorder: &DownloadSpeedRecorder,
) -> Result<(), String> {
    info!(
        "media gateway: parallel range enabled url={} start={} end={} total={} chunk={} connections={}",
        redact_url(remote_url),
        plan.response_start,
        plan.response_end,
        plan.total_size,
        PARALLEL_RANGE_CHUNK_BYTES,
        PARALLEL_RANGE_CONNECTIONS
    );

    let ranges = split_byte_ranges(plan.response_start, plan.response_end);
    let mut next_range_index = 1;
    let mut next_write_start = plan.response_start;
    let mut pending = FuturesUnordered::new();
    let mut completed: HashMap<u64, Vec<u8>> = HashMap::new();
    completed.insert(plan.response_start, first_chunk);

    loop {
        while pending.len() < PARALLEL_RANGE_CONNECTIONS && next_range_index < ranges.len() {
            let range = ranges[next_range_index].clone();
            next_range_index += 1;
            pending.push(fetch_range_bytes(
                app_handle,
                remote_url,
                range,
                download_speed_recorder,
            ));
        }

        if let Some(bytes) = completed.remove(&next_write_start) {
            stream
                .write_all(&bytes)
                .await
                .map_err(|error| error.to_string())?;
            next_write_start = next_write_start.saturating_add(bytes.len() as u64);
            if next_write_start > plan.response_end {
                return Ok(());
            }
            continue;
        }

        let Some(result) = pending.next().await else {
            return Ok(());
        };
        let (start, bytes) = result?;
        if start == next_write_start {
            stream
                .write_all(&bytes)
                .await
                .map_err(|error| error.to_string())?;
            next_write_start = next_write_start.saturating_add(bytes.len() as u64);
            if next_write_start > plan.response_end {
                return Ok(());
            }
        } else {
            completed.insert(start, bytes);
        }
    }
}

async fn stream_response(
    app_handle: &AppHandle,
    stream: &mut TcpStream,
    method: &str,
    remote_url: &str,
    request_range: Option<&str>,
    mut response: Response,
    download_speed_recorder: &DownloadSpeedRecorder,
) -> Result<(), String> {
    let status = response.status();
    let content_type = content_type(&response);
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let accept_ranges = response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "bytes".to_string());
    let parallel_plan = if method == "GET" && parallel_range_enabled() {
        parallel_range_plan(
            remote_url,
            request_range,
            status,
            content_length,
            content_range.as_deref(),
            &accept_ranges,
        )
    } else {
        None
    };
    let parallel_first_chunk = if let Some(plan) = parallel_plan.as_ref() {
        let first_range = ByteRange {
            start: plan.response_start,
            end: plan
                .response_start
                .saturating_add(PARALLEL_RANGE_CHUNK_BYTES - 1)
                .min(plan.response_end),
        };
        match fetch_range_bytes(app_handle, remote_url, first_range, download_speed_recorder).await {
            Ok((start, bytes)) if start == plan.response_start => Some(bytes),
            Ok((start, _)) => {
                warn!(
                    "media gateway: parallel range preflight returned unexpected start={} expected={} url={}",
                    start,
                    plan.response_start,
                    redact_url(remote_url)
                );
                None
            }
            Err(error) => {
                debug!(
                    "media gateway: parallel range disabled after preflight url={} error={}",
                    redact_url(remote_url),
                    error
                );
                None
            }
        }
    } else {
        None
    };

    write_response(
        stream,
        status.as_u16(),
        status.canonical_reason().unwrap_or("OK"),
        &content_type,
        content_length,
        content_range.as_deref(),
        Some(&accept_ranges),
    )
    .await?;

    if method == "HEAD" {
        return Ok(());
    }

    if let (Some(plan), Some(first_chunk)) = (parallel_plan, parallel_first_chunk) {
        drop(response);
        return stream_parallel_range_response(
            app_handle,
            stream,
            remote_url,
            plan,
            first_chunk,
            download_speed_recorder,
        )
        .await;
    }

    loop {
        let read_started_at = Instant::now();
        let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? else {
            break;
        };
        download_speed_recorder.record_transfer(
            chunk.len(),
            read_started_at,
            Instant::now(),
        );
        stream
            .write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn write_status(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    body: &[u8],
) -> Result<(), String> {
    write_response(
        stream,
        code,
        reason,
        "text/plain; charset=utf-8",
        Some(body.len() as u64),
        None,
        None,
    )
    .await?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())
}

async fn write_response(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_type: &str,
    content_length: Option<u64>,
    content_range: Option<&str>,
    accept_ranges: Option<&str>,
) -> Result<(), String> {
    let mut header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nConnection: close\r\n"
    );
    if let Some(length) = content_length {
        header.push_str(&format!("Content-Length: {length}\r\n"));
    }
    if let Some(range) = content_range {
        header.push_str(&format!("Content-Range: {range}\r\n"));
    }
    if let Some(accept_ranges) = accept_ranges {
        header.push_str(&format!("Accept-Ranges: {accept_ranges}\r\n"));
    }
    header.push_str("\r\n");
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        cast_media_leases, cleanup_expired_cast_media_leases, edl_stream_urls,
        handle_lan_connection_with_context,
        lookup_media_source, media_source_registry, parse_content_range, parse_request,
        infer_media_mime, parse_cast_media_token, parse_cast_media_route, parse_single_byte_range,
        rewrite_playlist, rewrite_playlist_for_endpoint, revoke_cast_media_session,
        CastMediaEndpoint, ResourceRegistration,
        DownloadSpeedMeter, DownloadSpeedMeterHandle, LocalFileMediaSourceBackend,
        MediaSourceBackend, MediaSourceRegistry, MEDIA_SOURCE_IDLE_TIMEOUT,
        LOOPBACK_MEDIA_BASE_URL,
    };
    use super::lease::CastMediaLease;
    use futures_util::future::BoxFuture;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tauri::AppHandle;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    struct TestBackend {
        origin: String,
        meter: DownloadSpeedMeterHandle,
    }

    impl TestBackend {
        fn new(origin: &str) -> Self {
            Self {
                origin: origin.to_string(),
                meter: Arc::new(Mutex::new(DownloadSpeedMeter::default())),
            }
        }
    }

    impl MediaSourceBackend for TestBackend {
        fn label(&self) -> &'static str { "test" }

        fn origin(&self) -> &str { &self.origin }

        fn download_speed_meter(&self) -> &DownloadSpeedMeterHandle { &self.meter }

        fn handle<'a>(
            &'a self,
            _app_handle: Option<&'a AppHandle>,
            _stream: &'a mut TcpStream,
            _method: &'a str,
            _range: Option<&'a str>,
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
    }

    async fn send_lan_media_request(client_ip: IpAddr, request: &str) -> String {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();
        let server = tokio::spawn(async move {
            handle_lan_connection_with_context(server_stream, client_ip, None).await
        });

        client.write_all(request.as_bytes()).await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        server.await.unwrap().unwrap();
        String::from_utf8(response).unwrap()
    }

    #[test]
    fn download_speed_meter_uses_a_bounded_rolling_window() {
        let start = Instant::now();
        let mut meter = DownloadSpeedMeter::default();
        let generation = meter.begin_generation_at(start);
        meter.record_transfer_at(
            generation,
            64 * 1024,
            start,
            start + Duration::from_millis(100),
        );
        assert_eq!(
            meter.speed_bps_at(start + Duration::from_millis(100)),
            256.0 * 1024.0,
        );

        meter.record_transfer_at(
            generation,
            64 * 1024,
            start + Duration::from_millis(100),
            start + Duration::from_millis(1100),
        );
        let speed = meter.speed_bps_at(start + Duration::from_millis(1100));
        assert!(speed > 100.0 * 1024.0);
        assert!(speed < 130.0 * 1024.0);
        assert_eq!(meter.speed_bps_at(start + Duration::from_millis(3201)), 0.0);
    }

    #[test]
    fn download_speed_meter_ignores_reads_from_an_old_generation() {
        let start = Instant::now();
        let mut meter = DownloadSpeedMeter::default();
        let old_generation = meter.begin_generation_at(start);
        let current_generation = meter.begin_generation_at(start + Duration::from_secs(1));
        meter.record_transfer_at(
            old_generation,
            64 * 1024,
            start + Duration::from_secs(1),
            start + Duration::from_millis(1100),
        );
        assert_eq!(meter.speed_bps_at(start + Duration::from_millis(1100)), 0.0);

        meter.record_transfer_at(
            current_generation,
            64 * 1024,
            start + Duration::from_millis(1100),
            start + Duration::from_millis(1200),
        );
        assert!(meter.speed_bps_at(start + Duration::from_millis(1200)) > 0.0);
    }

    #[test]
    fn download_speed_meter_starts_a_new_burst_after_becoming_stale() {
        let start = Instant::now();
        let mut meter = DownloadSpeedMeter::default();
        let generation = meter.begin_generation_at(start);
        meter.record_transfer_at(
            generation,
            64 * 1024,
            start,
            start + Duration::from_millis(100),
        );
        meter.record_transfer_at(
            generation,
            64 * 1024,
            start + Duration::from_millis(2101),
            start + Duration::from_millis(2201),
        );

        assert_eq!(
            meter.speed_bps_at(start + Duration::from_millis(2201)),
            256.0 * 1024.0,
        );
    }

    #[test]
    fn download_speed_meter_weights_transfers_crossing_the_window_boundary() {
        let start = Instant::now();
        let mut meter = DownloadSpeedMeter::default();
        let generation = meter.begin_generation_at(start);
        meter.record_transfer_at(
            generation,
            2 * 1024 * 1024,
            start + Duration::from_millis(400),
            start + Duration::from_millis(1300),
        );
        meter.record_transfer_at(
            generation,
            2 * 1024 * 1024,
            start + Duration::from_millis(1300),
            start + Duration::from_millis(2100),
        );

        let speed = meter.speed_bps_at(start + Duration::from_millis(2800));
        assert!(speed > 1.5 * 1024.0 * 1024.0);
        assert!(speed < 1.6 * 1024.0 * 1024.0);
    }

    #[test]
    fn edl_stream_urls_extracts_each_embedded_proxy_url() {
        let video = "http://127.0.0.1:31000/stream/video-token";
        let audio = "http://127.0.0.1:31000/stream/audio-token";
        let edl = format!(
            "edl://!new_stream;%{}%{};!new_stream;%{}%{}",
            video.len(),
            video,
            audio.len(),
            audio,
        );

        assert_eq!(edl_stream_urls(&edl), vec![video, audio]);
    }

    #[test]
    fn range_parser_characterizes_single_range_semantics() {
        assert_eq!(parse_single_byte_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_single_byte_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_single_byte_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_single_byte_range("bytes=-200", 100), Some((0, 99)));
        assert_eq!(parse_single_byte_range("bytes=100-", 100), None);
        assert_eq!(parse_single_byte_range("bytes=9-8", 100), None);
        assert_eq!(parse_single_byte_range("bytes=0-1,3-4", 100), None);
    }

    #[test]
    fn request_parser_keeps_head_and_range_contract() {
        let request = "HEAD /stream/token HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=10-\r\n\r\n";
        assert_eq!(
            parse_request(request).unwrap(),
            ("HEAD".to_string(), "/stream/token".to_string(), Some("bytes=10-".to_string()))
        );
        assert_eq!(parse_content_range("bytes 10-99/100"), Some((10, 99, 100)));
    }

    #[test]
    fn cast_route_accepts_only_an_exact_opaque_media_path() {
        assert_eq!(
            parse_cast_media_token("/cast/01234567-89ab-cdef-0123-456789abcdef/media"),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
        assert_eq!(parse_cast_media_token("/stream/source-token"), None);
        assert_eq!(parse_cast_media_token("/cast/token/media?url=https://example.test"), None);
        assert_eq!(parse_cast_media_token("/cast/token/other"), None);
        assert_eq!(parse_cast_media_token("/cast/token/nested/media"), None);
        let resource = parse_cast_media_route("/cast/lease-token/resource/source-token").unwrap();
        assert_eq!(resource.lease_token, "lease-token");
        assert_eq!(resource.source_id, Some("source-token"));
        assert!(parse_cast_media_route("/cast/lease-token/resource/source-token/extra").is_none());
    }

    #[test]
    fn local_file_backend_binds_one_file_and_detects_size_changes() {
        let path = std::env::temp_dir().join(format!("soia-cast-{}.mkv", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"initial-media-bytes").unwrap();
        let backend = LocalFileMediaSourceBackend::new(&path).unwrap();

        assert_eq!(backend.label(), "local-file");
        assert_eq!(infer_media_mime(&path), "video/x-matroska");
        assert!(!backend.origin().contains(path.to_string_lossy().as_ref()));
        assert!(backend.validate_file_state().is_ok());

        std::fs::write(&path, b"replacement-media-with-a-different-size").unwrap();
        assert!(backend.validate_file_state().is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn lan_local_file_endpoint_enforces_lease_and_range_contract() {
        let media_path = std::env::temp_dir().join(format!("soia-cast-{}.mp4", uuid::Uuid::new_v4()));
        std::fs::write(&media_path, b"0123456789abcdef").unwrap();
        let source_id = uuid::Uuid::new_v4().to_string();
        let session_id = format!("test-session-{}", uuid::Uuid::new_v4());
        let receiver_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20));
        media_source_registry()
            .lock()
            .unwrap()
            .insert(source_id.clone(), Arc::new(LocalFileMediaSourceBackend::new(&media_path).unwrap()));
        let lease = CastMediaLease::new(
            session_id.clone(),
            source_id.clone(),
            receiver_ip,
            Duration::from_secs(60),
        );
        let media_path_url = lease.media_path();
        cast_media_leases().lock().unwrap().insert(lease).unwrap();

        let head = send_lan_media_request(
            receiver_ip,
            &format!("HEAD {media_path_url} HTTP/1.1\r\nHost: receiver\r\n\r\n"),
        )
        .await;
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("Content-Length: 16\r\n"));
        assert!(head.ends_with("\r\n\r\n"));

        let range = send_lan_media_request(
            receiver_ip,
            &format!("GET {media_path_url} HTTP/1.1\r\nHost: receiver\r\nRange: bytes=4-7\r\n\r\n"),
        )
        .await;
        assert!(range.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        assert!(range.contains("Content-Range: bytes 4-7/16\r\n"));
        assert!(range.ends_with("\r\n\r\n4567"));

        let unsatisfiable = send_lan_media_request(
            receiver_ip,
            &format!("GET {media_path_url} HTTP/1.1\r\nHost: receiver\r\nRange: bytes=16-\r\n\r\n"),
        )
        .await;
        assert!(unsatisfiable.starts_with("HTTP/1.1 416 Range Not Satisfiable\r\n"));
        assert!(unsatisfiable.contains("Content-Range: bytes */16\r\n"));

        let rejected = send_lan_media_request(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 21)),
            &format!("GET {media_path_url} HTTP/1.1\r\nHost: receiver\r\n\r\n"),
        )
        .await;
        assert!(rejected.starts_with("HTTP/1.1 404 Not Found\r\n"));

        cast_media_leases().lock().unwrap().revoke_session(&session_id);
        media_source_registry().lock().unwrap().remove(&source_id);
        std::fs::remove_file(media_path).unwrap();
    }

    #[test]
    fn hls_rewrite_keeps_relative_segments_and_key_uris_behind_tokens() {
        LOOPBACK_MEDIA_BASE_URL
            .get_or_init(|| "http://127.0.0.1:39001".to_string());
        let meter = Arc::new(Mutex::new(DownloadSpeedMeter::default()));
        let playlist = rewrite_playlist(
            "https://media.example.test/live/master.m3u8",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"keys/session.key\"\nsegments/0001.ts",
            Some(&[("Referer".to_string(), "https://app.example.test/".to_string())]),
            &meter,
        );
        let playlist = playlist.unwrap();
        let rewritten = playlist.lines().collect::<Vec<_>>();
        let key_url = rewritten[1]
            .split("URI=\"")
            .nth(1)
            .unwrap()
            .trim_end_matches('"');
        let segment_url = rewritten[2];

        assert!(key_url.starts_with("http://127.0.0.1:39001/stream/"));
        assert!(segment_url.starts_with("http://127.0.0.1:39001/stream/"));
        let key_target = url::Url::parse(key_url).unwrap().path().to_string();
        let segment_target = url::Url::parse(segment_url).unwrap().path().to_string();
        assert_eq!(
            lookup_media_source(&key_target).unwrap().origin(),
            "https://media.example.test/live/keys/session.key"
        );
        assert_eq!(
            lookup_media_source(&segment_target).unwrap().origin(),
            "https://media.example.test/live/segments/0001.ts"
        );
    }

    #[test]
    fn cast_hls_rewrite_keeps_child_resources_inside_the_same_lease() {
        let lease_token = uuid::Uuid::new_v4().to_string();
        let session_id = format!("test-session-{}", uuid::Uuid::new_v4());
        let source_id = uuid::Uuid::new_v4().to_string();
        let receiver_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20));
        cast_media_leases()
            .lock()
            .unwrap()
            .insert(CastMediaLease::new_with_token(
                lease_token.clone(),
                session_id.clone(),
                source_id,
                receiver_ip,
                Duration::from_secs(60),
            ))
            .unwrap();
        let endpoint = CastMediaEndpoint {
            public_base_url: "http://192.0.2.10:39002".to_string(),
            lease_token: lease_token.clone(),
        };
        let meter = Arc::new(Mutex::new(DownloadSpeedMeter::default()));
        let playlist = rewrite_playlist_for_endpoint(
            "https://media.example.test/live/master.m3u8",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"keys/session.key\"\nsegments/0001.ts",
            Some(&[("Referer".to_string(), "https://app.example.test/".to_string())]),
            &meter,
            Some(&endpoint),
        )
        .unwrap();
        let repeated = rewrite_playlist_for_endpoint(
            "https://media.example.test/live/master.m3u8",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"keys/session.key\"\nsegments/0001.ts",
            Some(&[("Referer".to_string(), "https://app.example.test/".to_string())]),
            &meter,
            Some(&endpoint),
        )
        .unwrap();
        assert_eq!(playlist, repeated);
        let rewritten = playlist.lines().collect::<Vec<_>>();
        let key_url = rewritten[1]
            .split("URI=\"")
            .nth(1)
            .unwrap()
            .trim_end_matches('"');
        let segment_url = rewritten[2];
        for url in [key_url, segment_url] {
            let parsed = url::Url::parse(url).unwrap();
            assert_eq!(parsed.scheme(), "http");
            assert_eq!(parsed.host_str(), Some("192.0.2.10"));
            let route = parse_cast_media_route(parsed.path()).unwrap();
            assert_eq!(route.lease_token, lease_token);
            let child_source_id = route.source_id.unwrap();
            assert!(cast_media_leases()
                .lock()
                .unwrap()
                .authorize(&lease_token, receiver_ip)
                .unwrap()
                .contains_source(child_source_id));
            media_source_registry().lock().unwrap().remove(child_source_id);
        }
        cast_media_leases().lock().unwrap().revoke_session(&session_id);
    }

    #[test]
    fn cast_session_revocation_releases_primary_and_child_media_sources() {
        let primary_source = uuid::Uuid::new_v4().to_string();
        let child_source = uuid::Uuid::new_v4().to_string();
        let session_id = format!("test-session-{}", uuid::Uuid::new_v4());
        let receiver_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20));
        {
            let mut sources = media_source_registry().lock().unwrap();
            sources.insert(primary_source.clone(), Arc::new(TestBackend::new("https://example.test/master.m3u8")));
            sources.insert(child_source.clone(), Arc::new(TestBackend::new("https://example.test/segment.ts")));
        }
        let lease = CastMediaLease::new(
            session_id.clone(),
            primary_source.clone(),
            receiver_ip,
            Duration::from_secs(60),
        );
        let lease_token = lease.token.clone();
        cast_media_leases().lock().unwrap().insert(lease).unwrap();
        assert!(matches!(cast_media_leases()
            .lock()
            .unwrap()
            .register_resource(
                &lease_token,
                "https://example.test/segment.ts".to_string(),
                child_source.clone(),
            ),
            ResourceRegistration::Registered,
        ));

        revoke_cast_media_session(&session_id);

        let mut sources = media_source_registry().lock().unwrap();
        assert!(sources.get(&primary_source).is_none());
        assert!(sources.get(&child_source).is_none());
    }

    #[test]
    fn expired_cast_lease_releases_its_media_source() {
        let source_id = uuid::Uuid::new_v4().to_string();
        let session_id = format!("test-session-{}", uuid::Uuid::new_v4());
        let receiver_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20));
        media_source_registry()
            .lock()
            .unwrap()
            .insert(source_id.clone(), Arc::new(TestBackend::new("https://example.test/expired.mp4")));
        cast_media_leases()
            .lock()
            .unwrap()
            .insert(CastMediaLease::new(
                session_id,
                source_id.clone(),
                receiver_ip,
                Duration::ZERO,
            ))
            .unwrap();

        cleanup_expired_cast_media_leases();

        assert!(media_source_registry().lock().unwrap().get(&source_id).is_none());
    }

    #[test]
    fn backend_registry_reuses_origin_and_removes_idle_entries() {
        let mut registry = MediaSourceRegistry::new();
        registry.insert("token-a".to_string(), Arc::new(TestBackend::new("https://example.test/a.mp4")));

        assert_eq!(
            registry
                .find_token_by_origin("https://example.test/a.mp4")
                .as_deref(),
            Some("token-a")
        );
        assert_eq!(registry.get("token-a").unwrap().label(), "test");

        let now = Instant::now();
        registry.entries.get_mut("token-a").unwrap().last_access = now - MEDIA_SOURCE_IDLE_TIMEOUT - Duration::from_secs(1);
        registry.cleanup_idle(now);
        assert!(registry.get("token-a").is_none());
    }
}
