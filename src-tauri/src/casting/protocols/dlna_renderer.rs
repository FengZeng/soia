use super::super::{
    CastAdapterSession, CastMediaDescriptor, CastProtocolAdapter, CastProtocolCommand,
    CastReceiverStatus,
};
use crate::casting::discovery::ssdp;
use futures_util::future::{join_all, BoxFuture};
use roxmltree::Document;
use soia_protocol::{
    CastCapabilitiesDto, CastDeviceDto, CastErrorCodeDto, CastErrorDto, CastProtocolDto,
};
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const DLNA_RENDERER_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const DESCRIPTION_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct DlnaRendererAdapter;

impl DlnaRendererAdapter {
    pub(crate) fn new() -> Self { Self }
}

impl CastProtocolAdapter for DlnaRendererAdapter {
    fn protocol(&self) -> CastProtocolDto { CastProtocolDto::Dlna }

    fn discover<'a>(&'a self) -> BoxFuture<'a, Result<Vec<CastDeviceDto>, CastErrorDto>> {
        Box::pin(async move {
            let responses = ssdp::discover(DLNA_RENDERER_TARGET, DISCOVERY_TIMEOUT)
                .await
                .map_err(|error| discovery_error(&error))?;
            let client = reqwest::Client::builder()
                .timeout(DESCRIPTION_TIMEOUT)
                .build()
                .map_err(|error| discovery_error(&error.to_string()))?;
            let descriptions = join_all(responses.into_iter().map(|response| {
                let client = client.clone();
                async move { discover_renderer(&client, response).await }
            }))
            .await;

            let mut devices = descriptions
                .into_iter()
                .filter_map(|result| match result {
                    Ok(device) => Some(device),
                    Err(error) => {
                        log::debug!("DLNA MediaRenderer response ignored: {}", error);
                        None
                    }
                })
                .collect::<Vec<_>>();
            devices.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));
            devices.dedup_by(|left, right| left.id == right.id);
            Ok(devices)
        })
    }

    fn connect<'a>(
        &'a self,
        device: &'a CastDeviceDto,
    ) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>> {
        Box::pin(async move {
            Err(device_error(
                CastErrorCodeDto::ConnectionFailed,
                "DLNA playback control is not available yet",
                Some(&device.id),
            ))
        })
    }

    fn load<'a>(
        &'a self,
        session: &'a CastAdapterSession,
        _media: &'a CastMediaDescriptor,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            Err(device_error(
                CastErrorCodeDto::LoadFailed,
                "DLNA playback control is not available yet",
                Some(&session.device_id),
            ))
        })
    }

    fn command<'a>(
        &'a self,
        session: &'a CastAdapterSession,
        _command: CastProtocolCommand,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            Err(device_error(
                CastErrorCodeDto::CommandFailed,
                "DLNA playback control is not available yet",
                Some(&session.device_id),
            ))
        })
    }

    fn status<'a>(
        &'a self,
        session: &'a CastAdapterSession,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            Err(device_error(
                CastErrorCodeDto::CommandFailed,
                "DLNA playback control is not available yet",
                Some(&session.device_id),
            ))
        })
    }

    fn disconnect<'a>(
        &'a self,
        _session: &'a CastAdapterSession,
    ) -> BoxFuture<'a, Result<(), CastErrorDto>> {
        Box::pin(async { Ok(()) })
    }
}

async fn discover_renderer(
    client: &reqwest::Client,
    response: ssdp::SsdpResponse,
) -> Result<CastDeviceDto, String> {
    if !ssdp::response_matches_target(&response, DLNA_RENDERER_TARGET) {
        return Err("SSDP response is not a MediaRenderer".to_string());
    }
    let location = response
        .location
        .as_deref()
        .ok_or_else(|| "missing LOCATION header".to_string())?;
    let location = Url::parse(location).map_err(|_| "invalid LOCATION URL".to_string())?;
    let address = location_ipv4_for_response(&location, response.source.ip())?;
    let body = client
        .get(location)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|_| "device description request failed".to_string())?
        .text()
        .await
        .map_err(|_| "device description response could not be read".to_string())?;
    let description = parse_renderer_description(&body)?;
    let id = description
        .udn
        .or_else(|| response.usn.as_deref().and_then(udn_from_usn).map(str::to_string))
        .ok_or_else(|| "device description has no UDN".to_string())?;

    Ok(CastDeviceDto {
        id,
        protocol: CastProtocolDto::Dlna,
        name: description.name.unwrap_or_else(|| "DLNA Renderer".to_string()),
        model_name: description.model_name,
        address: address.to_string(),
        capabilities: conservative_capabilities(),
        last_seen_at: unix_time_secs(),
    })
}

struct RendererDescription {
    udn: Option<String>,
    name: Option<String>,
    model_name: Option<String>,
}

fn parse_renderer_description(xml: &str) -> Result<RendererDescription, String> {
    let document = Document::parse(xml).map_err(|_| "invalid device description XML".to_string())?;
    let device = document
        .descendants()
        .find(|node| {
            node.is_element()
                && node.tag_name().name().eq_ignore_ascii_case("device")
                && child_text(*node, "deviceType")
                    .is_some_and(|device_type| device_type.eq_ignore_ascii_case(DLNA_RENDERER_TARGET))
        })
        .ok_or_else(|| "description is not a DLNA MediaRenderer".to_string())?;
    Ok(RendererDescription {
        udn: child_text(device, "UDN"),
        name: child_text(device, "friendlyName"),
        model_name: child_text(device, "modelName"),
    })
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name().eq_ignore_ascii_case(name))
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn location_ipv4_for_response(location: &Url, source: IpAddr) -> Result<Ipv4Addr, String> {
    if !matches!(location.scheme(), "http" | "https") || !location.username().is_empty() || location.password().is_some() {
        return Err("LOCATION must be an unauthenticated HTTP URL".to_string());
    }
    let source = match source {
        IpAddr::V4(address) => address,
        IpAddr::V6(_) => return Err("renderer response did not use IPv4".to_string()),
    };
    let location_address = location
        .host_str()
        .and_then(|host| host.parse::<Ipv4Addr>().ok())
        .ok_or_else(|| "LOCATION host is not an IPv4 address".to_string())?;
    if location_address != source {
        return Err("LOCATION host does not match the SSDP responder".to_string());
    }
    Ok(location_address)
}

fn udn_from_usn(usn: &str) -> Option<&str> {
    usn.split("::").next().map(str::trim).filter(|value| !value.is_empty())
}

fn unix_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

/// Discovery alone does not prove the SOAP services or formats needed for a control are usable.
/// GetProtocolInfo and the service capability checks promote these values in later milestones.
fn conservative_capabilities() -> CastCapabilitiesDto {
    CastCapabilitiesDto::default()
}

fn discovery_error(message: &str) -> CastErrorDto {
    device_error(
        CastErrorCodeDto::DiscoveryFailed,
        &format!("DLNA discovery failed: {message}"),
        None,
    )
}

fn device_error(code: CastErrorCodeDto, message: &str, device_id: Option<&str>) -> CastErrorDto {
    CastErrorDto {
        code,
        message: message.to_string(),
        device_id: device_id.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        conservative_capabilities, location_ipv4_for_response, parse_renderer_description,
        udn_from_usn,
    };
    use std::net::{IpAddr, Ipv4Addr};
    use url::Url;

    #[test]
    fn parses_a_renderer_description_without_accepting_a_media_server() {
        let renderer = parse_renderer_description(include_str!("../fixtures/dlna_renderer/description.xml")).unwrap();
        assert_eq!(renderer.udn.as_deref(), Some("uuid:00000000-0000-0000-0000-000000000000"));
        assert_eq!(renderer.name.as_deref(), Some("Redacted DLNA Renderer"));
        assert_eq!(renderer.model_name.as_deref(), Some("Linux UPnP MediaRenderer"));

        let server = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType></device></root>"#;
        assert!(parse_renderer_description(server).is_err());
    }

    #[test]
    fn requires_an_ipv4_location_from_the_ssdp_responder() {
        let location = Url::parse("http://192.0.2.20:25826/description.xml").unwrap();
        assert_eq!(
            location_ipv4_for_response(&location, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20))).unwrap(),
            Ipv4Addr::new(192, 0, 2, 20),
        );
        assert!(location_ipv4_for_response(&location, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 21))).is_err());
        assert!(location_ipv4_for_response(&Url::parse("https://example.test/description.xml").unwrap(), IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20))).is_err());
    }

    #[test]
    fn extracts_the_udn_from_a_renderer_usn() {
        assert_eq!(
            udn_from_usn("uuid:renderer-1::urn:schemas-upnp-org:device:MediaRenderer:1"),
            Some("uuid:renderer-1"),
        );
    }

    #[test]
    fn discovery_does_not_advertise_controls_before_service_validation() {
        let capabilities = conservative_capabilities();

        assert!(!capabilities.play);
        assert!(!capabilities.pause);
        assert!(!capabilities.seek);
        assert!(!capabilities.stop);
        assert!(!capabilities.volume);
    }
}
