mod event_loop;
mod ffi;
mod handle;
mod series_match;
mod ytdlp_resolver;
mod ytdlp_settings;
mod stream_https;

#[cfg(target_os = "macos")]
pub(crate) use ffi::SoiaUtils;
pub(crate) use handle::MpvHandle;
pub(crate) use ytdlp_resolver::try_resolve as try_resolve_with_ytdlp;
pub(crate) use ytdlp_resolver::resolve_for_cast as resolve_ytdlp_for_cast;
pub(crate) use ytdlp_resolver::ResolvedCastStreams;
pub(crate) use ytdlp_resolver::resolve_playlist as resolve_ytdlp_playlist;
pub(crate) use ytdlp_settings::store_runtime_settings as store_runtime_ytdlp_settings;
pub(crate) use ytdlp_settings::YtdlpFormatSettings;
pub(crate) use ytdlp_settings::YtdlpSettings;

pub(crate) const USE_SMB_STREAM_PROXY: bool = true;
pub(crate) const USE_WEBDAV_STREAM_PROXY: bool = true;

pub(crate) fn register_stream_basic_auth(playback_url: &str, username: &str, password: &str) {
    crate::media_gateway::register_basic_auth(playback_url, username, password);
    stream_https::register_basic_auth(playback_url, username, password);
}

pub(crate) fn prepare_network_stream_url(
    protocol: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty() {
        return Ok(url.to_string());
    }

    register_stream_basic_auth(url, username, password);
    let protocol = protocol.trim().to_ascii_lowercase();
    if matches!(protocol.as_str(), "smb" | "samba") && !USE_SMB_STREAM_PROXY {
        return crate::network::protocols::smb::playback_url_with_credentials(
            url,
            username,
            password,
        );
    }

    Ok(url.to_string())
}

pub(crate) fn rewrite_network_stream_url(protocol: &str, url: &str) -> Option<String> {
    let protocol = protocol.trim().to_ascii_lowercase();
    match protocol.as_str() {
        "webdav" => {
            if USE_WEBDAV_STREAM_PROXY {
                crate::media_gateway::create_loopback_http_media_url(url)
                    .or_else(|| crate::media_gateway::create_loopback_https_media_url(url))
            } else {
                rewrite_https_callback_url(url)
                    .or_else(|| crate::media_gateway::create_loopback_http_media_url(url))
            }
        }
        "smb" | "samba" => {
            if USE_SMB_STREAM_PROXY {
                crate::media_gateway::create_loopback_smb_media_url(url)
            } else {
                None
            }
        }
        _ => crate::media_gateway::create_loopback_http_media_url(url)
            .or_else(|| crate::media_gateway::create_loopback_https_media_url(url))
            .or_else(|| crate::media_gateway::create_loopback_smb_media_url(url))
            .or_else(|| rewrite_https_callback_url(url)),
    }
}

pub(crate) fn rewrite_https_callback_url(url: &str) -> Option<String> {
    stream_https::rewrite_https_callback_url(url)
}

#[cfg(target_os = "android")]
pub(crate) fn register_java_vm(vm: *mut std::ffi::c_void) -> Result<(), String> {
    let ret = unsafe { ffi::av_jni_set_java_vm(vm, std::ptr::null_mut()) };
    if ret < 0 {
        Err(format!("av_jni_set_java_vm failed with error code: {ret}"))
    } else {
        Ok(())
    }
}
