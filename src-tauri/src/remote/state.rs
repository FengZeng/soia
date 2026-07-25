use qrcode::QrCode;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const PAIR_CODE_TTL: Duration = Duration::from_secs(99);

pub(super) static REMOTE_CONTROL_ADDR: OnceLock<SocketAddr> = OnceLock::new();
pub(super) static REMOTE_CONTROL_TOKEN: OnceLock<String> = OnceLock::new();
pub(super) static REMOTE_CONTROL_RUNTIME: OnceLock<Arc<Mutex<RemoteControlRuntime>>> =
    OnceLock::new();

pub(super) struct RemoteControlRuntime {
    pub(super) enabled: bool,
    pub(super) pair_code: Option<(String, Instant)>,
    pub(super) sessions: std::collections::HashSet<String>,
}

#[derive(Clone)]
pub(super) struct RemoteControlState {
    pub(super) app_handle: tauri::AppHandle,
    pub(super) token: Option<String>,
    pub(super) web_root: PathBuf,
    pub(super) runtime: Arc<Mutex<RemoteControlRuntime>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteControlInfo {
    pub url: String,
    pub qr_svg: String,
    pub enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteControlStatus {
    enabled: bool,
    connected_devices: usize,
}

pub(super) fn resolve_auth_token(token_env_var: &str) -> Option<String> {
    std::env::var(token_env_var)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .or_else(|| Some(uuid::Uuid::now_v7().to_string()))
}

pub(crate) fn get_remote_control_info() -> Result<RemoteControlInfo, String> {
    let addr = REMOTE_CONTROL_ADDR
        .get()
        .ok_or_else(|| "Remote control service is not running".to_string())?;
    let runtime = remote_runtime()?;
    let pair_code = {
        let mut runtime = runtime.lock().map_err(|error| error.to_string())?;
        if !runtime.enabled {
            return Err("Web Remote is disabled".to_string());
        }
        let pair_code = uuid::Uuid::now_v7().to_string();
        runtime.pair_code = Some((pair_code.clone(), Instant::now() + PAIR_CODE_TTL));
        pair_code
    };
    let host = if addr.ip().is_unspecified() {
        local_network_ip()?
    } else {
        addr.ip().to_string()
    };
    let url = format!("http://{host}:{}/remote/#pair={pair_code}", addr.port());
    let qr_svg = QrCode::new(url.as_bytes())
        .map_err(|error| error.to_string())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(qrcode::render::svg::Color("#15151b"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    Ok(RemoteControlInfo {
        url,
        qr_svg,
        enabled: true,
    })
}

pub(crate) fn get_remote_control_status() -> Result<RemoteControlStatus, String> {
    let runtime = remote_runtime()?;
    let runtime = runtime.lock().map_err(|error| error.to_string())?;
    Ok(RemoteControlStatus {
        enabled: runtime.enabled,
        connected_devices: runtime.sessions.len(),
    })
}

pub(crate) fn set_remote_control_enabled(enabled: bool) -> Result<RemoteControlStatus, String> {
    let runtime = remote_runtime()?;
    let mut runtime = runtime.lock().map_err(|error| error.to_string())?;
    runtime.enabled = enabled;
    runtime.pair_code = None;
    if !enabled {
        runtime.sessions.clear();
    }
    Ok(RemoteControlStatus {
        enabled: runtime.enabled,
        connected_devices: runtime.sessions.len(),
    })
}

pub(crate) fn disconnect_remote_control_devices() -> Result<RemoteControlStatus, String> {
    let runtime = remote_runtime()?;
    let mut runtime = runtime.lock().map_err(|error| error.to_string())?;
    runtime.pair_code = None;
    runtime.sessions.clear();
    Ok(RemoteControlStatus {
        enabled: runtime.enabled,
        connected_devices: 0,
    })
}

pub(super) fn remote_runtime() -> Result<&'static Arc<Mutex<RemoteControlRuntime>>, String> {
    REMOTE_CONTROL_RUNTIME
        .get()
        .ok_or_else(|| "Remote control service is not running".to_string())
}

pub(super) fn is_enabled(state: &RemoteControlState) -> bool {
    state
        .runtime
        .lock()
        .map(|runtime| runtime.enabled)
        .unwrap_or(false)
}

pub(super) fn is_active_session(state: &RemoteControlState, session: &str) -> bool {
    state
        .runtime
        .lock()
        .map(|runtime| runtime.sessions.contains(session))
        .unwrap_or(false)
}

pub(super) fn is_connection_active(
    state: &RemoteControlState,
    session: Option<&str>,
) -> bool {
    is_enabled(state) && session.is_none_or(|session| is_active_session(state, session))
}

fn local_network_ip() -> Result<String, String> {
    let mut candidates: Vec<(u8, std::net::Ipv4Addr)> = if_addrs::get_if_addrs()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|interface| !is_virtual_interface(&interface.name))
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(address) => Some(address.ip),
            if_addrs::IfAddr::V6(_) => None,
        })
        .filter(|ip| is_usable_private_ipv4(*ip))
        .map(|ip| (private_ip_priority(ip), ip))
        .collect();

    candidates.sort_by_key(|(priority, ip)| (*priority, *ip));
    candidates
        .first()
        .map(|(_, ip)| ip.to_string())
        .ok_or_else(|| "No private local network address found".to_string())
}

fn is_virtual_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["lo", "utun", "tun", "tap", "docker", "vbox", "vmnet", "bridge"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn is_usable_private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    ip.is_private()
        && !ip.is_loopback()
        && !ip.is_unspecified()
        && !(ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
}

fn private_ip_priority(ip: std::net::Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        [172, 16..=31, ..] => 2,
        _ => 3,
    }
}
