use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::time;

const SSDP_MULTICAST_ADDR: &str = "239.255.255.250:1900";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SsdpResponse {
    pub source: SocketAddr,
    pub usn: Option<String>,
    pub location: Option<String>,
    pub server: Option<String>,
    pub search_target: Option<String>,
}

pub(crate) async fn discover(
    search_target: &str,
    timeout: Duration,
) -> Result<Vec<SsdpResponse>, String> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|error| format!("failed to bind SSDP socket: {error}"))?;
    let target = SSDP_MULTICAST_ADDR
        .to_socket_addrs()
        .map_err(|error| format!("failed to resolve SSDP multicast address: {error}"))?
        .next()
        .ok_or_else(|| "SSDP multicast address has no socket address".to_string())?;
    let request = build_m_search_request(search_target);

    socket
        .send_to(request.as_bytes(), target)
        .await
        .map_err(|error| format!("failed to send SSDP discovery request: {error}"))?;

    let deadline = Instant::now() + timeout.max(Duration::from_secs(1));
    let mut responses = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut buffer = [0u8; 8192];
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(Ok((length, source))) = time::timeout(remaining, socket.recv_from(&mut buffer)).await else {
            break;
        };
        let packet = String::from_utf8_lossy(&buffer[..length]);
        let Some(response) = parse_response(&packet, source) else {
            continue;
        };
        let dedup_key = (
            response.usn.clone().unwrap_or_default(),
            response.location.clone().unwrap_or_default(),
        );
        if seen.insert(dedup_key) {
            responses.push(response);
        }
    }
    Ok(responses)
}

fn build_m_search_request(search_target: &str) -> String {
    [
        "M-SEARCH * HTTP/1.1".to_string(),
        format!("HOST: {SSDP_MULTICAST_ADDR}"),
        "MAN: \"ssdp:discover\"".to_string(),
        "MX: 2".to_string(),
        format!("ST: {search_target}"),
        String::new(),
        String::new(),
    ]
    .join("\r\n")
}

fn parse_response(packet: &str, source: SocketAddr) -> Option<SsdpResponse> {
    let mut lines = packet.lines();
    let status_line = lines.next()?.trim();
    if !status_line.starts_with("HTTP/") || !status_line.contains(" 200") {
        return None;
    }
    let headers = parse_headers(lines);
    Some(SsdpResponse {
        source,
        usn: headers.get("usn").cloned(),
        location: headers.get("location").cloned(),
        server: headers.get("server").cloned(),
        search_target: headers.get("st").cloned(),
    })
}

fn parse_headers<'a>(lines: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    lines
        .filter_map(|line| {
            let (name, value) = line.trim().split_once(':')?;
            let value = value.trim();
            (!value.is_empty()).then(|| (name.trim().to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

pub(crate) fn response_matches_target(response: &SsdpResponse, search_target: &str) -> bool {
    response
        .search_target
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case(search_target))
}

#[cfg(test)]
mod tests {
    use super::{build_m_search_request, parse_response, response_matches_target};
    use std::net::SocketAddr;

    const RENDERER_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

    #[test]
    fn builds_a_renderer_m_search_request() {
        let request = build_m_search_request(RENDERER_TARGET);

        assert!(request.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(request.contains("MAN: \"ssdp:discover\"\r\n"));
        assert!(request.contains("MX: 2\r\n"));
        assert!(request.contains(&format!("ST: {RENDERER_TARGET}\r\n")));
        assert!(request.ends_with("\r\n\r\n"));
    }

    #[test]
    fn parses_case_insensitive_response_headers() {
        let response = parse_response(
            include_str!("../fixtures/dlna_renderer/ssdp-response.txt"),
            "192.0.2.20:1900".parse::<SocketAddr>().unwrap(),
        )
        .unwrap();

        assert_eq!(response.location.as_deref(), Some("http://192.0.2.20:25826/description.xml"));
        assert_eq!(response.usn.as_deref(), Some("uuid:00000000-0000-0000-0000-000000000000::urn:schemas-upnp-org:device:MediaRenderer:1"));
        assert!(response_matches_target(&response, RENDERER_TARGET));
    }

    #[test]
    fn rejects_non_response_packets_and_other_search_targets() {
        let source = "192.0.2.20:1900".parse::<SocketAddr>().unwrap();
        assert!(parse_response("NOTIFY * HTTP/1.1\r\n\r\n", source).is_none());

        let response = parse_response(
            "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n",
            source,
        )
        .unwrap();
        assert!(!response_matches_target(&response, RENDERER_TARGET));
    }
}
