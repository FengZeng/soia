use super::ffi::{soia_utils_register_stream_protocol, SoiaStreamCbInfo, SoiaUtils};
use log::{info, warn};
use percent_encoding::percent_decode_str;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{HeaderMap, ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, RANGE, USER_AGENT};
use reqwest::StatusCode;
use std::ffi::{c_void, CStr, CString};
use std::io::Read;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::AppHandle;

const MPV_ERROR_LOADING_FAILED: c_int = -13;
const MPV_ERROR_UNSUPPORTED: i64 = -18;
const MPV_ERROR_GENERIC: i64 = -20;
const MPV_ERROR_NOMEM: c_int = -2;
const MPV_ERROR_INVALID_PARAMETER: c_int = -4;
const READ_CAP: u64 = 1024 * 1024;
const HTTP_USER_AGENT: &str = "Lavf/61.7.100";

type BasicAuth = (String, String);

static HTTPS_BASIC_AUTH: OnceLock<Mutex<std::collections::HashMap<String, BasicAuth>>> =
    OnceLock::new();

pub(crate) struct HttpsStreamRegistry {
    app_handle: AppHandle,
}

struct HttpsStream {
    client: Client,
    url: String,
    log_url: String,
    basic_auth: Option<(String, String)>,
    position: u64,
    size: Option<u64>,
    response: Option<Response>,
    logged_first_read: bool,
    canceled: AtomicBool,
}

// Legacy callback path kept for the old soia-https stream protocol. New remote playback should
// register credentials with media_gateway and expose only localhost token URLs to mpv.
#[allow(dead_code)]
pub(crate) fn register_basic_auth(playback_url: &str, username: &str, password: &str) {
    let username = username.trim();
    if username.is_empty() {
        return;
    }
    let auth = (username.to_string(), password.to_string());
    if let Ok(mut auth_map) = HTTPS_BASIC_AUTH
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
    {
        auth_map.insert(playback_url.to_string(), auth);
        if let Some(suffix) = playback_url.strip_prefix("soia-https://") {
            auth_map.insert(
                format!("https://{suffix}"),
                (username.to_string(), password.to_string()),
            );
        }
    }
}

pub(crate) fn rewrite_https_callback_url(url: &str) -> Option<String> {
    url.strip_prefix("https://")
        .map(|suffix| format!("soia-https://{suffix}"))
}

impl HttpsStreamRegistry {
    pub(crate) fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    pub(crate) fn register(&mut self, utils: *mut SoiaUtils) -> Result<(), String> {
        register_protocol(utils, "soia-https", self)
    }
}

fn register_protocol(
    utils: *mut SoiaUtils,
    protocol: &str,
    registry: &mut HttpsStreamRegistry,
) -> Result<(), String> {
    let c_protocol = CString::new(protocol).map_err(|_| "Invalid stream protocol".to_string())?;
    let ret = unsafe {
        soia_utils_register_stream_protocol(
            utils,
            c_protocol.as_ptr(),
            registry as *mut HttpsStreamRegistry as *mut c_void,
            Some(https_stream_open_ro),
        )
    };
    match ret {
        Some(code) if code < 0 => Err(format!(
            "Failed to register {protocol} stream callback: {code}"
        )),
        Some(_) => {
            info!("https stream: registered Rust mpv stream callback protocol={protocol}");
            Ok(())
        }
        None => {
            Err("https stream: soia_utils_register_stream_protocol is unavailable; rebuild soia_utils".to_string())
        }
    }
}

fn build_client(app_handle: &AppHandle) -> Result<Client, String> {
    let builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(30));
    crate::network::proxy::configure_blocking_client_builder(app_handle, builder)?
        .build()
        .map_err(|error| error.to_string())
}

fn parse_content_range_size(value: &str) -> Option<u64> {
    let size_part = value.rsplit_once('/')?.1.trim();
    if size_part == "*" {
        None
    } else {
        size_part.parse::<u64>().ok()
    }
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn content_range_size(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range_size)
}

fn redact_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return raw.to_string();
    };
    if !url.username().is_empty() {
        let _ = url.set_username("<user>");
        let _ = url.set_password(Some("<redacted>"));
    }
    url.to_string()
}

fn take_registered_basic_auth(raw: &str, request_url: &str) -> Option<BasicAuth> {
    HTTPS_BASIC_AUTH
        .get()
        .and_then(|auth_map| auth_map.lock().ok())
        .and_then(|mut auth_map| {
            auth_map
                .remove(raw)
                .or_else(|| auth_map.remove(request_url))
        })
}

fn extract_basic_auth(raw: &str) -> (String, String, Option<BasicAuth>) {
    let Ok(mut url) = url::Url::parse(raw) else {
        return (raw.to_string(), raw.to_string(), None);
    };
    let username = percent_decode_str(url.username())
        .decode_utf8_lossy()
        .to_string();
    if username.is_empty() {
        let log_url = url.to_string();
        return (log_url.clone(), log_url, None);
    }

    let password = url
        .password()
        .map(|value| percent_decode_str(value).decode_utf8_lossy().to_string())
        .unwrap_or_default();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let request_url = url.to_string();
    (request_url.clone(), request_url, Some((username, password)))
}

fn apply_basic_auth(request: RequestBuilder, auth: &Option<(String, String)>) -> RequestBuilder {
    match auth {
        Some((username, password)) => request.basic_auth(username, Some(password)),
        None => request,
    }
}

impl HttpsStream {
    fn open(app_handle: &AppHandle, url: String) -> Result<Self, String> {
        let client = build_client(app_handle)?;
        let raw_url = url;
        let (url, log_url, embedded_basic_auth) = extract_basic_auth(&raw_url);
        let basic_auth = embedded_basic_auth.or_else(|| take_registered_basic_auth(&raw_url, &url));
        info!(
            "https stream: Rust auth {} url={}",
            if basic_auth.is_some() { "found" } else { "missing" },
            log_url
        );
        let size = apply_basic_auth(
            client.head(&url).header(ACCEPT_ENCODING, "identity"),
            &basic_auth,
        )
            .send()
            .ok()
            .and_then(|response| {
                response
                    .status()
                    .is_success()
                    .then(|| content_length(response.headers()))
                    .flatten()
            });

        Ok(Self {
            client,
            url,
            log_url,
            basic_auth,
            position: 0,
            size,
            response: None,
            logged_first_read: false,
            canceled: AtomicBool::new(false),
        })
    }

    fn ensure_response(&mut self) -> Result<(), String> {
        if self.response.is_some() {
            return Ok(());
        }
        if self.canceled.load(Ordering::Acquire) {
            return Err("HTTPS stream was canceled".to_string());
        }

        let use_range = self.position > 0;
        let mut request = self
            .client
            .get(&self.url)
            .header(ACCEPT_ENCODING, "identity")
            .header(USER_AGENT, HTTP_USER_AGENT);
        if use_range {
            request = request.header(RANGE, format!("bytes={}-", self.position));
        }
        let response = apply_basic_auth(request, &self.basic_auth)
            .send()
            .map_err(|error| error.to_string())?;

        let status = response.status();
        if status == StatusCode::PARTIAL_CONTENT {
            self.size = self.size.or_else(|| content_range_size(response.headers()));
            info!(
                "https stream: opened Rust response url={} position={} status={} size={:?}",
                self.log_url, self.position, status, self.size
            );
            self.response = Some(response);
            return Ok(());
        }
        if status == StatusCode::OK && self.position == 0 {
            self.size = self.size.or_else(|| content_length(response.headers()));
            info!(
                "https stream: opened Rust response url={} position={} status={} size={:?}",
                self.log_url, self.position, status, self.size
            );
            self.response = Some(response);
            return Ok(());
        }

        Err(format!(
            "HTTP request failed: status={status} range={} position={}",
            if use_range { "yes" } else { "no" },
            self.position
        ))
    }

    fn read_into(&mut self, buf: *mut c_char, nbytes: u64) -> i64 {
        if buf.is_null() {
            return MPV_ERROR_GENERIC;
        }
        if nbytes == 0 {
            return 0;
        }
        if self.canceled.load(Ordering::Acquire) {
            return MPV_ERROR_GENERIC;
        }
        if let Err(error) = self.ensure_response() {
            warn!("https stream: read setup failed for {}: {}", self.log_url, error);
            return MPV_ERROR_GENERIC;
        }

        let count = nbytes.min(READ_CAP) as usize;
        let slice = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), count) };
        let read_result = self
            .response
            .as_mut()
            .map(|response| response.read(slice))
            .unwrap_or_else(|| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "missing response",
                ))
            });

        match read_result {
            Ok(bytes_read) => {
                if !self.logged_first_read {
                    info!(
                        "https stream: Rust read active url={} bytes={} position={}",
                        self.log_url, bytes_read, self.position
                    );
                    self.logged_first_read = true;
                }
                self.position = self.position.saturating_add(bytes_read as u64);
                bytes_read as i64
            }
            Err(error) => {
                warn!("https stream: read failed for {}: {}", self.log_url, error);
                MPV_ERROR_GENERIC
            }
        }
    }

    fn seek(&mut self, offset: i64) -> i64 {
        if offset < 0 {
            return MPV_ERROR_GENERIC;
        }
        if self.canceled.load(Ordering::Acquire) {
            return MPV_ERROR_GENERIC;
        }
        info!("https stream: Rust seek url={} offset={}", self.log_url, offset);
        self.position = offset as u64;
        self.response = None;
        offset
    }
}

unsafe extern "C" fn https_stream_open_ro(
    userdata: *mut c_void,
    uri: *mut c_char,
    info: *mut SoiaStreamCbInfo,
) -> c_int {
    if userdata.is_null() || uri.is_null() || info.is_null() {
        return MPV_ERROR_INVALID_PARAMETER;
    }

    let registry = &*(userdata as *mut HttpsStreamRegistry);
    let url = match CStr::from_ptr(uri).to_str() {
        Ok(value) if value.starts_with("soia-https://") => {
            format!("https://{}", &value["soia-https://".len()..])
        }
        Ok(value) if value.starts_with("https://") => value.to_string(),
        Ok(_) => return MPV_ERROR_INVALID_PARAMETER,
        Err(_) => return MPV_ERROR_INVALID_PARAMETER,
    };
    info!("https stream: Rust open callback url={}", redact_url(&url));

    let stream = match HttpsStream::open(&registry.app_handle, url) {
        Ok(stream) => stream,
        Err(error) => {
            warn!("https stream: open failed: {}", error);
            return MPV_ERROR_LOADING_FAILED;
        }
    };
    let cookie = Box::into_raw(Box::new(stream));
    if cookie.is_null() {
        return MPV_ERROR_NOMEM;
    }

    (*info).cookie = cookie.cast::<c_void>();
    (*info).read_fn = Some(https_stream_read);
    (*info).seek_fn = Some(https_stream_seek);
    (*info).size_fn = Some(https_stream_size);
    (*info).close_fn = Some(https_stream_close);
    (*info).cancel_fn = Some(https_stream_cancel);
    0
}

unsafe extern "C" fn https_stream_read(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64 {
    if cookie.is_null() {
        return MPV_ERROR_GENERIC;
    }
    let stream = &mut *(cookie as *mut HttpsStream);
    stream.read_into(buf, nbytes)
}

unsafe extern "C" fn https_stream_seek(cookie: *mut c_void, offset: i64) -> i64 {
    if cookie.is_null() {
        return MPV_ERROR_GENERIC;
    }
    let stream = &mut *(cookie as *mut HttpsStream);
    stream.seek(offset)
}

unsafe extern "C" fn https_stream_size(cookie: *mut c_void) -> i64 {
    if cookie.is_null() {
        return MPV_ERROR_UNSUPPORTED;
    }
    let stream = &*(cookie as *mut HttpsStream);
    stream
        .size
        .and_then(|size| i64::try_from(size).ok())
        .unwrap_or(MPV_ERROR_UNSUPPORTED)
}

unsafe extern "C" fn https_stream_close(cookie: *mut c_void) {
    if !cookie.is_null() {
        drop(Box::from_raw(cookie as *mut HttpsStream));
    }
}

unsafe extern "C" fn https_stream_cancel(cookie: *mut c_void) {
    if cookie.is_null() {
        return;
    }
    let stream = &*(cookie as *mut HttpsStream);
    stream.canceled.store(true, Ordering::Release);
}
