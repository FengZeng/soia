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
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const DLNA_RENDERER_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const DESCRIPTION_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct DlnaRendererAdapter {
    descriptions: Mutex<HashMap<String, RendererDescription>>,
}

impl DlnaRendererAdapter {
    pub(crate) fn new() -> Self {
        Self {
            descriptions: Mutex::new(HashMap::new()),
        }
    }
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

            let mut discovered = descriptions
                .into_iter()
                .filter_map(|result| match result {
                    Ok(renderer) => Some(renderer),
                    Err(error) => {
                        log::debug!("DLNA MediaRenderer response ignored: {}", error);
                        None
                    }
                })
                .collect::<Vec<_>>();
            discovered.sort_by(|left, right| {
                left.device
                    .name
                    .cmp(&right.device.name)
                    .then_with(|| left.device.id.cmp(&right.device.id))
            });
            discovered.dedup_by(|left, right| left.device.id == right.device.id);
            let descriptions = discovered
                .iter()
                .map(|renderer| (renderer.device.id.clone(), renderer.description.clone()))
                .collect();
            *self
                .descriptions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = descriptions;
            let devices = discovered
                .into_iter()
                .map(|renderer| renderer.device)
                .collect();
            Ok(devices)
        })
    }

    fn connect<'a>(
        &'a self,
        device: &'a CastDeviceDto,
    ) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>> {
        Box::pin(async move {
            let description = self
                .descriptions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&device.id)
                .cloned()
                .ok_or_else(|| {
                    device_error(
                        CastErrorCodeDto::ConnectionFailed,
                        "DLNA device description is no longer available; scan again",
                        Some(&device.id),
                    )
                })?;
            if description.services.av_transport.is_none() {
                return Err(device_error(
                    CastErrorCodeDto::DeviceUnsupported,
                    "DLNA device does not advertise AVTransport",
                    Some(&device.id),
                ));
            }
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
) -> Result<DiscoveredRenderer, String> {
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
        .get(location.clone())
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|_| "device description request failed".to_string())?
        .text()
        .await
        .map_err(|_| "device description response could not be read".to_string())?;
    let description = parse_renderer_description(&location, &body)?;
    let id = description
        .udn
        .clone()
        .or_else(|| response.usn.as_deref().and_then(udn_from_usn).map(str::to_string))
        .ok_or_else(|| "device description has no UDN".to_string())?;

    Ok(DiscoveredRenderer {
        device: CastDeviceDto {
            id,
            protocol: CastProtocolDto::Dlna,
            name: description.name.clone().unwrap_or_else(|| "DLNA Renderer".to_string()),
            model_name: description.model_name.clone(),
            address: address.to_string(),
            capabilities: conservative_capabilities(),
            last_seen_at: unix_time_secs(),
        },
        description,
    })
}

struct DiscoveredRenderer {
    device: CastDeviceDto,
    description: RendererDescription,
}

#[derive(Clone)]
struct RendererDescription {
    udn: Option<String>,
    name: Option<String>,
    model_name: Option<String>,
    services: RendererServices,
}

#[derive(Clone, Default)]
struct RendererServices {
    av_transport: Option<DlnaService>,
    rendering_control: Option<DlnaService>,
    connection_manager: Option<DlnaService>,
}

#[derive(Clone)]
struct DlnaService {
    service_type: String,
    control_url: Url,
}

fn parse_renderer_description(
    description_url: &Url,
    xml: &str,
) -> Result<RendererDescription, String> {
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
        services: parse_renderer_services(device, description_url)?,
    })
}

fn parse_renderer_services(
    device: roxmltree::Node<'_, '_>,
    description_url: &Url,
) -> Result<RendererServices, String> {
    let mut services = RendererServices::default();
    for service in device.descendants().filter(|node| {
        node.is_element() && node.tag_name().name().eq_ignore_ascii_case("service")
    }) {
        let Some(service_type) = child_text(service, "serviceType") else {
            continue;
        };
        let Some(kind) = service_kind(&service_type) else {
            continue;
        };
        let Some(control_url) = child_text(service, "controlURL") else {
            continue;
        };
        let control_url = resolve_control_url(description_url, &control_url)?;
        let service = DlnaService {
            service_type,
            control_url,
        };
        match kind {
            DlnaServiceKind::AvTransport => services.av_transport = Some(service),
            DlnaServiceKind::RenderingControl => services.rendering_control = Some(service),
            DlnaServiceKind::ConnectionManager => services.connection_manager = Some(service),
        }
    }
    Ok(services)
}

enum DlnaServiceKind {
    AvTransport,
    RenderingControl,
    ConnectionManager,
}

fn service_kind(service_type: &str) -> Option<DlnaServiceKind> {
    let service_name = service_type.split(':').nth(3)?;
    if service_name.eq_ignore_ascii_case("AVTransport") {
        return Some(DlnaServiceKind::AvTransport);
    }
    if service_name.eq_ignore_ascii_case("RenderingControl") {
        return Some(DlnaServiceKind::RenderingControl);
    }
    if service_name.eq_ignore_ascii_case("ConnectionManager") {
        return Some(DlnaServiceKind::ConnectionManager);
    }
    None
}

fn resolve_control_url(description_url: &Url, value: &str) -> Result<Url, String> {
    let control_url = description_url
        .join(value.trim())
        .map_err(|_| "invalid service control URL".to_string())?;
    if !matches!(control_url.scheme(), "http" | "https")
        || !control_url.username().is_empty()
        || control_url.password().is_some()
    {
        return Err("service control URL must be an unauthenticated HTTP URL".to_string());
    }
    Ok(control_url)
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
        let description_url = Url::parse("http://192.0.2.20:25826/description.xml").unwrap();
        let renderer = parse_renderer_description(
            &description_url,
            include_str!("../fixtures/dlna_renderer/description.xml"),
        )
        .unwrap();
        assert_eq!(renderer.udn.as_deref(), Some("uuid:00000000-0000-0000-0000-000000000000"));
        assert_eq!(renderer.name.as_deref(), Some("Redacted DLNA Renderer"));
        assert_eq!(renderer.model_name.as_deref(), Some("Linux UPnP MediaRenderer"));
        assert_eq!(
            renderer
                .services
                .av_transport
                .as_ref()
                .map(|service| service.control_url.as_str()),
            Some("http://192.0.2.20:25826/upnp/service/AVTransport/Control"),
        );
        assert_eq!(
            renderer
                .services
                .rendering_control
                .as_ref()
                .map(|service| service.control_url.as_str()),
            Some("http://192.0.2.20:25826/upnp/service/RenderingControl/Control"),
        );
        assert_eq!(
            renderer
                .services
                .connection_manager
                .as_ref()
                .map(|service| service.control_url.as_str()),
            Some("http://192.0.2.20:25826/upnp/service/ConnectionManager/Control"),
        );

        let server = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType></device></root>"#;
        assert!(parse_renderer_description(&description_url, server).is_err());
    }

    #[test]
    fn resolves_relative_control_urls_against_the_description_url() {
        let description_url = Url::parse("http://192.0.2.20:25826/upnp/description.xml").unwrap();
        let xml = r#"
            <root><device>
              <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
              <serviceList><service>
                <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
                <controlURL>control/avtransport</controlURL>
              </service></serviceList>
            </device></root>
        "#;
        let renderer = parse_renderer_description(&description_url, xml).unwrap();

        assert_eq!(
            renderer
                .services
                .av_transport
                .as_ref()
                .map(|service| service.control_url.as_str()),
            Some("http://192.0.2.20:25826/upnp/control/avtransport"),
        );
    }

    #[test]
    fn ignores_unrelated_services_with_an_invalid_control_url() {
        let description_url = Url::parse("http://192.0.2.20:25826/description.xml").unwrap();
        let xml = r#"
            <root><device>
              <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
              <serviceList>
                <service>
                  <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
                  <controlURL>/upnp/avtransport</controlURL>
                </service>
                <service>
                  <serviceType>urn:example-org:service:Unrelated:1</serviceType>
                  <controlURL>mailto:invalid@example.test</controlURL>
                </service>
              </serviceList>
            </device></root>
        "#;

        assert!(parse_renderer_description(&description_url, xml).is_ok());
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
