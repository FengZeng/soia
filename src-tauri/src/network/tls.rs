use crate::store::network_connection_store::NetworkConnectionRecord;
use reqwest::{Client, ClientBuilder, Url};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use std::net::IpAddr;
use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct PinnedCertificateVerifier {
    certificate: Vec<u8>,
}

impl ServerCertVerifier for PinnedCertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() == self.certificate.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "the server certificate no longer matches the certificate saved for this connection"
                    .to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if value.len() % 2 != 0 {
        return Err("saved TLS certificate is invalid".to_string());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair)
                .map_err(|_| "saved TLS certificate is invalid".to_string())?;
            u8::from_str_radix(pair, 16)
                .map_err(|_| "saved TLS certificate is invalid".to_string())
        })
        .collect()
}

fn encode_hex(value: &[u8]) -> String {
    use std::fmt::Write;

    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn pinned_tls_config(certificate_hex: &str) -> Result<rustls::ClientConfig, String> {
    let verifier = PinnedCertificateVerifier {
        certificate: decode_hex(certificate_hex)?,
    };
    Ok(rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth())
}

fn contains_certificate_error(error: &(dyn StdError + 'static), depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    if matches!(
        error.downcast_ref::<RustlsError>(),
        Some(RustlsError::InvalidCertificate(_))
    ) {
        return true;
    }
    if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
        if io_error
            .get_ref()
            .is_some_and(|inner| contains_certificate_error(inner, depth + 1))
        {
            return true;
        }
    }
    error
        .source()
        .is_some_and(|source| contains_certificate_error(source, depth + 1))
}

pub(crate) fn is_certificate_error(error: &reqwest::Error) -> bool {
    contains_certificate_error(error, 0)
}

pub(crate) fn no_tls_downgrade_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 10 {
            return attempt.error("too many redirects");
        }
        let previous = attempt.previous().last().expect("redirect has a previous URL");
        if previous.scheme() == "https" && attempt.url().scheme() != "https" {
            return attempt.error("HTTPS server redirected to an insecure URL");
        }
        attempt.follow()
    })
}

fn pinned_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 10 {
            return attempt.error("too many redirects");
        }
        let original = attempt.previous().first().expect("redirect has an original URL");
        let next = attempt.url();
        if original.scheme() != "https"
            || next.scheme() != "https"
            || original.host_str() != next.host_str()
            || original.port_or_known_default() != next.port_or_known_default()
        {
            return attempt.error("pinned HTTPS server redirected to another origin");
        }
        attempt.follow()
    })
}

pub(crate) fn configure_pinned_client_builder(
    builder: ClientBuilder,
    certificate_hex: Option<&str>,
) -> Result<ClientBuilder, String> {
    match certificate_hex.map(str::trim).filter(|value| !value.is_empty()) {
        Some(certificate_hex) => Ok(builder
            .use_preconfigured_tls(pinned_tls_config(certificate_hex)?)
            .redirect(pinned_redirects())
            .no_proxy()),
        None => Ok(builder),
    }
}

pub(crate) fn configure_pinned_blocking_client_builder(
    builder: reqwest::blocking::ClientBuilder,
    certificate_hex: Option<&str>,
) -> Result<reqwest::blocking::ClientBuilder, String> {
    match certificate_hex.map(str::trim).filter(|value| !value.is_empty()) {
        Some(certificate_hex) => Ok(builder
            .use_preconfigured_tls(pinned_tls_config(certificate_hex)?)
            .redirect(pinned_redirects())
            .no_proxy()),
        None => Ok(builder),
    }
}

fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unicast_link_local()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

async fn ensure_local_direct_target(app: &tauri::AppHandle, url: &Url) -> Result<(), String> {
    if crate::network::proxy::current_proxy_key(app)?.is_some() {
        return Err("automatic trust is disabled while a network proxy is configured".to_string());
    }
    if url.scheme() != "https" {
        return Err("automatic certificate trust is only available for HTTPS".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "HTTPS server address has no host".to_string())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "HTTPS server address has no port".to_string())?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("failed to resolve HTTPS server address: {error}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|address| !is_local_ip(address.ip())) {
        return Err("automatic trust is only available for local network servers".to_string());
    }
    Ok(())
}

pub(crate) async fn trust_local_certificate(
    app: &tauri::AppHandle,
    connection: &NetworkConnectionRecord,
    target_url: &Url,
) -> Result<String, String> {
    ensure_local_direct_target(app, target_url).await?;

    // This probe deliberately carries no WebDAV credentials. Redirects are disabled so an
    // untrusted endpoint cannot move the probe to a different host before it is pinned.
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(true)
        .tls_info(true)
        .no_proxy()
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .head(target_url.clone())
        .send()
        .await
        .map_err(|error| format!("failed to inspect local HTTPS certificate: {error}"))?;
    let peer_address = response
        .remote_addr()
        .ok_or_else(|| "could not verify the local HTTPS server address".to_string())?;
    if !is_local_ip(peer_address.ip()) {
        return Err("automatic trust is only available for local network servers".to_string());
    }
    let certificate = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(reqwest::tls::TlsInfo::peer_certificate)
        .ok_or_else(|| "local HTTPS server did not provide a certificate".to_string())?;
    let certificate_hex = encode_hex(certificate);
    crate::store::network_connection_store::save_tls_certificate(
        app,
        &connection.id,
        &connection.base_url,
        &certificate_hex,
    )?;
    log::info!(
        "trusted first-use TLS certificate for local WebDAV connection id={}",
        connection.id
    );
    Ok(certificate_hex)
}

pub(crate) async fn ensure_connection_certificate(
    app: &tauri::AppHandle,
    connection: &NetworkConnectionRecord,
    target_url: &Url,
) -> Result<NetworkConnectionRecord, String> {
    if connection.tls_certificate_der.is_some() || target_url.scheme() != "https" {
        return Ok(connection.clone());
    }

    let builder = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    let strict_client = crate::network::proxy::configure_client_builder(app, builder)?
        .build()
        .map_err(|error| error.to_string())?;
    match strict_client.head(target_url.clone()).send().await {
        Ok(_) => return Ok(connection.clone()),
        Err(error) if !is_certificate_error(&error) => return Err(error.to_string()),
        Err(_) => {}
    }

    let certificate = trust_local_certificate(app, connection, target_url).await?;
    let mut trusted = connection.clone();
    trusted.tls_certificate_der = Some(certificate);
    Ok(trusted)
}

#[cfg(test)]
mod tests {
    use super::{contains_certificate_error, decode_hex, encode_hex, is_local_ip};
    use std::net::IpAddr;

    #[test]
    fn identifies_local_addresses() {
        for value in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.2.3",
            "192.168.1.2",
            "169.254.1.2",
            "::1",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(is_local_ip(value.parse::<IpAddr>().unwrap()), "{value}");
        }
        for value in ["8.8.8.8", "172.15.2.3", "2001:4860:4860::8888"] {
            assert!(!is_local_ip(value.parse::<IpAddr>().unwrap()), "{value}");
        }
    }

    #[test]
    fn certificate_hex_round_trips() {
        let certificate = [0, 1, 15, 16, 127, 128, 255];
        assert_eq!(decode_hex(&encode_hex(&certificate)).unwrap(), certificate);
    }

    #[test]
    fn finds_certificate_errors_inside_nested_io_errors() {
        let certificate_error = std::io::Error::new(
            std::io::ErrorKind::Other,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
            ),
        );
        assert!(contains_certificate_error(&certificate_error, 0));

        let timeout = std::io::Error::new(std::io::ErrorKind::TimedOut, "request timed out");
        assert!(!contains_certificate_error(&timeout, 0));
    }
}
