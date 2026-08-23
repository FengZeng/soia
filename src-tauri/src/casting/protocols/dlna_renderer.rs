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
use tokio::time::{sleep, Instant};
use url::Url;

const DLNA_RENDERER_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const DESCRIPTION_TIMEOUT: Duration = Duration::from_secs(2);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_CONFIRM_INTERVAL: Duration = Duration::from_millis(250);

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
            if let Some(connection_manager) = description.services.connection_manager {
                match control_client() {
                    Ok(client) => match get_protocol_info(&client, &connection_manager).await {
                        Ok(sink_protocols) => {
                            self.cache_sink_protocols(&device.id, sink_protocols);
                        }
                        Err(error) => {
                            log::debug!(
                                "DLNA MediaRenderer GetProtocolInfo unavailable for {}: {}",
                                device.id,
                                error
                            );
                            self.cache_sink_protocols(&device.id, None);
                        }
                    },
                    Err(error) => {
                        log::debug!(
                            "DLNA MediaRenderer control client unavailable for {}: {}",
                            device.id,
                            error
                        );
                        self.cache_sink_protocols(&device.id, None);
                    }
                }
            }
            Ok(CastAdapterSession {
                id: uuid::Uuid::new_v4().to_string(),
                device_id: device.id.clone(),
            })
        })
    }

    fn load<'a>(
        &'a self,
        session: &'a CastAdapterSession,
        media: &'a CastMediaDescriptor,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let description = self.description_for(&session.device_id)?;
            let av_transport = description.services.av_transport.ok_or_else(|| {
                device_error(
                    CastErrorCodeDto::LoadFailed,
                    "DLNA device does not advertise AVTransport",
                    Some(&session.device_id),
                )
            })?;
            if let (Some(sink_protocols), Some(mime_type)) =
                (description.sink_protocols.as_ref(), media.mime_type.as_deref())
            {
                if !sink_protocols.supports_mime_type(mime_type) {
                    return Err(device_error(
                        CastErrorCodeDto::MediaUnavailable,
                        "DLNA device does not advertise support for this media format",
                        Some(&session.device_id),
                    ));
                }
            }
            let client = control_client().map_err(|error| {
                device_error(
                    CastErrorCodeDto::LoadFailed,
                    &error,
                    Some(&session.device_id),
                )
            })?;
            set_av_transport_uri(&client, &av_transport, media)
                .await
                .map_err(|error| {
                    device_error(CastErrorCodeDto::LoadFailed, &error, Some(&session.device_id))
                })?;
            play(&client, &av_transport).await.map_err(|error| {
                device_error(CastErrorCodeDto::LoadFailed, &error, Some(&session.device_id))
            })?;
            if media.position > 0.0 {
                seek_absolute(&client, &av_transport, media.position)
                    .await
                    .map_err(|error| {
                        device_error(CastErrorCodeDto::LoadFailed, &error, Some(&session.device_id))
                    })?;
            }
            let phase = wait_for_load_confirmation(&client, &av_transport)
                .await
                .map_err(|error| {
                    device_error(CastErrorCodeDto::LoadFailed, &error, Some(&session.device_id))
                })?;
            Ok(CastReceiverStatus {
                phase,
                position: media.position,
                duration: media.duration,
                volume: None,
                muted: None,
                seekable: false,
            })
        })
    }

    fn command<'a>(
        &'a self,
        session: &'a CastAdapterSession,
        command: CastProtocolCommand,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let description = self.description_for(&session.device_id)?;
            let rendering_control = description.services.rendering_control.clone();
            let av_transport = description.services.av_transport.ok_or_else(|| {
                device_error(
                    CastErrorCodeDto::CommandFailed,
                    "DLNA device does not advertise AVTransport",
                    Some(&session.device_id),
                )
            })?;
            let client = control_client().map_err(|error| {
                device_error(
                    CastErrorCodeDto::CommandFailed,
                    &error,
                    Some(&session.device_id),
                )
            })?;
            match command {
                CastProtocolCommand::Play => {
                    play(&client, &av_transport).await
                }
                CastProtocolCommand::Pause => {
                    pause(&client, &av_transport).await
                }
                CastProtocolCommand::Stop => {
                    stop(&client, &av_transport).await
                }
                CastProtocolCommand::SeekAbsolute { position } => {
                    seek_absolute(&client, &av_transport, position).await
                }
                CastProtocolCommand::SeekRelative { seconds } => {
                    if !seconds.is_finite() || !(-600.0..=600.0).contains(&seconds) {
                        return Err(device_error(
                            CastErrorCodeDto::CommandFailed,
                            "DLNA relative seek must be finite and within 600 seconds",
                            Some(&session.device_id),
                        ));
                    }
                    let position = get_position_info(&client, &av_transport)
                        .await
                        .map_err(|error| {
                            device_error(
                                CastErrorCodeDto::CommandFailed,
                                &error,
                                Some(&session.device_id),
                            )
                        })?
                        .0
                        .ok_or_else(|| {
                            device_error(
                                CastErrorCodeDto::CommandFailed,
                                "DLNA device did not report a seek position",
                                Some(&session.device_id),
                            )
                        })?;
                    seek_absolute(&client, &av_transport, (position + seconds).max(0.0))
                        .await
                }
                CastProtocolCommand::SetVolume { volume } => {
                    let rendering_control = rendering_control.as_ref().ok_or_else(|| {
                        device_error(
                            CastErrorCodeDto::CommandFailed,
                            "DLNA device does not advertise RenderingControl",
                            Some(&session.device_id),
                        )
                    })?;
                    set_volume(&client, rendering_control, volume).await.map(|_| ())
                }
                CastProtocolCommand::SetMuted { .. } => {
                    return Err(device_error(
                        CastErrorCodeDto::CommandFailed,
                        "this DLNA playback command is not available yet",
                        Some(&session.device_id),
                    ));
                }
            }
            .map_err(|error| {
                device_error(CastErrorCodeDto::CommandFailed, &error, Some(&session.device_id))
            })?;
            read_receiver_status(&client, &av_transport, rendering_control.as_ref())
                .await
                .map_err(|error| {
                    device_error(CastErrorCodeDto::CommandFailed, &error, Some(&session.device_id))
                })
        })
    }

    fn status<'a>(
        &'a self,
        session: &'a CastAdapterSession,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let description = self.description_for(&session.device_id)?;
            let av_transport = description.services.av_transport.ok_or_else(|| {
                device_error(
                    CastErrorCodeDto::CommandFailed,
                    "DLNA device does not advertise AVTransport",
                    Some(&session.device_id),
                )
            })?;
            let client = control_client().map_err(|error| {
                device_error(
                    CastErrorCodeDto::CommandFailed,
                    &error,
                    Some(&session.device_id),
                )
            })?;
            read_receiver_status(&client, &av_transport, description.services.rendering_control.as_ref())
                .await
                .map_err(|error| {
                    device_error(CastErrorCodeDto::CommandFailed, &error, Some(&session.device_id))
                })
        })
    }

    fn disconnect<'a>(
        &'a self,
        session: &'a CastAdapterSession,
    ) -> BoxFuture<'a, Result<(), CastErrorDto>> {
        Box::pin(async move {
            if let Ok(description) = self.description_for(&session.device_id) {
                if let Some(av_transport) = description.services.av_transport {
                    if let Ok(client) = control_client() {
                        let _ = stop(&client, &av_transport).await;
                    }
                }
            }
            Ok(())
        })
    }
}

impl DlnaRendererAdapter {
    fn cache_sink_protocols(&self, device_id: &str, sink_protocols: Option<SinkProtocolInfo>) {
        let Ok(mut descriptions) = self.descriptions.lock() else {
            return;
        };
        if let Some(description) = descriptions.get_mut(device_id) {
            description.sink_protocols = sink_protocols;
        }
    }

    fn description_for(&self, device_id: &str) -> Result<RendererDescription, CastErrorDto> {
        self.descriptions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(device_id)
            .cloned()
            .ok_or_else(|| {
                device_error(
                    CastErrorCodeDto::LoadFailed,
                    "DLNA device description is no longer available; scan again",
                    Some(device_id),
                )
            })
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
    sink_protocols: Option<SinkProtocolInfo>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SinkProtocolInfo {
    entries: Vec<DlnaProtocolInfo>,
}

impl SinkProtocolInfo {
    fn supports_mime_type(&self, mime_type: &str) -> bool {
        let mime_type = mime_type.trim();
        !mime_type.is_empty()
            && self.entries.iter().any(|entry| {
                entry.transport.eq_ignore_ascii_case("http-get")
                    && (entry.content_format == "*"
                    || entry.content_format.eq_ignore_ascii_case(mime_type)
                    )
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DlnaProtocolInfo {
    transport: String,
    network: String,
    content_format: String,
    additional_info: String,
}

async fn get_protocol_info(
    client: &reqwest::Client,
    service: &DlnaService,
) -> Result<Option<SinkProtocolInfo>, String> {
    let body = post_soap(
        client,
        service,
        "GetProtocolInfo",
        build_get_protocol_info_envelope(&service.service_type),
    )
    .await?;
    parse_get_protocol_info_response(&body)
}

async fn set_av_transport_uri(
    client: &reqwest::Client,
    service: &DlnaService,
    media: &CastMediaDescriptor,
) -> Result<(), String> {
    post_soap(
        client,
        service,
        "SetAVTransportURI",
        build_set_av_transport_uri_envelope(&service.service_type, media),
    )
    .await
    .map(|_| ())
}

async fn play(client: &reqwest::Client, service: &DlnaService) -> Result<(), String> {
    post_soap(
        client,
        service,
        "Play",
        build_play_envelope(&service.service_type),
    )
    .await
    .map(|_| ())
}

async fn pause(client: &reqwest::Client, service: &DlnaService) -> Result<(), String> {
    post_soap(
        client,
        service,
        "Pause",
        build_pause_envelope(&service.service_type),
    )
    .await
    .map(|_| ())
}

async fn stop(client: &reqwest::Client, service: &DlnaService) -> Result<(), String> {
    post_soap(
        client,
        service,
        "Stop",
        build_stop_envelope(&service.service_type),
    )
    .await
    .map(|_| ())
}

async fn seek_absolute(
    client: &reqwest::Client,
    service: &DlnaService,
    position: f64,
) -> Result<(), String> {
    let target = format_dlna_rel_time(position)?;
    post_soap(
        client,
        service,
        "Seek",
        build_seek_envelope(&service.service_type, &target),
    )
    .await
    .map(|_| ())
}

async fn wait_for_load_confirmation(
    client: &reqwest::Client,
    av_transport: &DlnaService,
) -> Result<soia_protocol::CastPhaseDto, String> {
    let deadline = Instant::now() + LOAD_CONFIRM_TIMEOUT;
    loop {
        let last_error = match get_transport_phase(client, av_transport).await {
            Ok(phase @ (soia_protocol::CastPhaseDto::Playing
            | soia_protocol::CastPhaseDto::Buffering
            | soia_protocol::CastPhaseDto::Paused)) => return Ok(phase),
            Ok(phase) => format!("receiver reports {phase:?} after media load"),
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            return Err(last_error);
        }
        sleep(LOAD_CONFIRM_INTERVAL).await;
    }
}

async fn read_receiver_status(
    client: &reqwest::Client,
    av_transport: &DlnaService,
    rendering_control: Option<&DlnaService>,
) -> Result<CastReceiverStatus, String> {
    let phase = get_transport_phase(client, av_transport).await?;
    let (position, duration) = match get_position_info(client, av_transport).await {
        Ok(status) => status,
        Err(error) => {
            log::debug!("DLNA GetPositionInfo unavailable: {error}");
            (None, None)
        }
    };
    let volume = match rendering_control {
        Some(service) => match get_volume(client, service).await {
            Ok(volume) => volume,
            Err(error) => {
                log::debug!("DLNA GetVolume unavailable: {error}");
                None
            }
        },
        None => None,
    };
    Ok(CastReceiverStatus {
        phase,
        position: position.unwrap_or_default(),
        duration,
        volume,
        muted: None,
        seekable: false,
    })
}

async fn get_transport_phase(
    client: &reqwest::Client,
    service: &DlnaService,
) -> Result<soia_protocol::CastPhaseDto, String> {
    let body = post_soap(
        client,
        service,
        "GetTransportInfo",
        build_get_transport_info_envelope(&service.service_type),
    )
    .await?;
    parse_get_transport_info_response(&body)
}

async fn get_position_info(
    client: &reqwest::Client,
    service: &DlnaService,
) -> Result<(Option<f64>, Option<f64>), String> {
    let body = post_soap(
        client,
        service,
        "GetPositionInfo",
        build_get_position_info_envelope(&service.service_type),
    )
    .await?;
    parse_get_position_info_response(&body)
}

async fn get_volume(client: &reqwest::Client, service: &DlnaService) -> Result<Option<f64>, String> {
    let body = post_soap(
        client,
        service,
        "GetVolume",
        build_get_volume_envelope(&service.service_type),
    )
    .await?;
    parse_get_volume_response(&body)
}

async fn set_volume(
    client: &reqwest::Client,
    service: &DlnaService,
    volume: f64,
) -> Result<f64, String> {
    if !volume.is_finite() {
        return Err("DLNA volume must be finite".to_string());
    }
    let volume = volume.clamp(0.0, 100.0).round();
    post_soap(
        client,
        service,
        "SetVolume",
        build_set_volume_envelope(&service.service_type, volume as u8),
    )
    .await
    .map(|_| volume)
}

async fn post_soap(
    client: &reqwest::Client,
    service: &DlnaService,
    action: &str,
    envelope: String,
) -> Result<String, String> {
    let response = client
        .post(service.control_url.clone())
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{}#{action}\"", service.service_type))
        .body(envelope)
        .send()
        .await
        .map_err(|_| format!("{action} request failed"))?
        .error_for_status()
        .map_err(|_| format!("{action} was rejected"))?;
    response
        .text()
        .await
        .map_err(|_| format!("{action} response could not be read"))
}

fn control_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(CONTROL_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "DLNA control client could not be created".to_string())
}

fn build_get_protocol_info_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetProtocolInfo xmlns:u="{service_type}" />
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_get_transport_info_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetTransportInfo xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetTransportInfo>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_get_position_info_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetPositionInfo xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetPositionInfo>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_get_volume_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetVolume xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
    </u:GetVolume>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_set_volume_envelope(service_type: &str, volume: u8) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetVolume xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
      <DesiredVolume>{volume}</DesiredVolume>
    </u:SetVolume>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_set_av_transport_uri_envelope(
    service_type: &str,
    media: &CastMediaDescriptor,
) -> String {
    let mime_type = media.mime_type.as_deref().unwrap_or("application/octet-stream");
    let didl = build_didl_lite(media, mime_type);
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetAVTransportURI xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <CurrentURI>{}</CurrentURI>
      <CurrentURIMetaData>{}</CurrentURIMetaData>
    </u:SetAVTransportURI>
  </s:Body>
</s:Envelope>"#,
        xml_escape(&media.url),
        xml_escape(&didl),
    )
}

fn build_play_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Play xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Speed>1</Speed>
    </u:Play>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_pause_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Pause xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:Pause>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_stop_envelope(service_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Stop xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:Stop>
  </s:Body>
</s:Envelope>"#,
    )
}

fn build_seek_envelope(service_type: &str, target: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Seek xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Unit>REL_TIME</Unit>
      <Target>{}</Target>
    </u:Seek>
  </s:Body>
</s:Envelope>"#,
        xml_escape(target),
    )
}

fn format_dlna_rel_time(position: f64) -> Result<String, String> {
    if !position.is_finite() || position < 0.0 {
        return Err("DLNA seek position must be a non-negative finite number".to_string());
    }
    let seconds = position.floor() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    Ok(format!("{hours:02}:{minutes:02}:{seconds:02}"))
}

fn build_didl_lite(media: &CastMediaDescriptor, mime_type: &str) -> String {
    let title = media.title.as_deref().unwrap_or("Soia media");
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="0" parentID="0" restricted="1"><dc:title>{}</dc:title><upnp:class>object.item.videoItem</upnp:class><res protocolInfo="http-get:*:{}:*">{}</res></item></DIDL-Lite>"#,
        xml_escape(title),
        xml_escape(mime_type),
        xml_escape(&media.url),
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse_get_protocol_info_response(xml: &str) -> Result<Option<SinkProtocolInfo>, String> {
    let document = Document::parse(xml).map_err(|_| "invalid GetProtocolInfo response XML".to_string())?;
    let sink = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case("Sink"))
        .and_then(|node| node.text())
        .unwrap_or_default();
    let entries = parse_protocol_info_list(sink);
    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SinkProtocolInfo { entries }))
    }
}

fn parse_get_transport_info_response(xml: &str) -> Result<soia_protocol::CastPhaseDto, String> {
    let state = soap_response_text(xml, "CurrentTransportState")?;
    if state.eq_ignore_ascii_case("PLAYING") {
        return Ok(soia_protocol::CastPhaseDto::Playing);
    }
    if state.eq_ignore_ascii_case("PAUSED_PLAYBACK") || state.eq_ignore_ascii_case("PAUSED_RECORDING") {
        return Ok(soia_protocol::CastPhaseDto::Paused);
    }
    if state.eq_ignore_ascii_case("TRANSITIONING") {
        return Ok(soia_protocol::CastPhaseDto::Buffering);
    }
    if state.eq_ignore_ascii_case("STOPPED") || state.eq_ignore_ascii_case("NO_MEDIA_PRESENT") {
        return Ok(soia_protocol::CastPhaseDto::Stopped);
    }
    Err(format!("unknown DLNA transport state: {state}"))
}

fn parse_get_position_info_response(xml: &str) -> Result<(Option<f64>, Option<f64>), String> {
    Ok((
        soap_response_text(xml, "RelTime")
            .ok()
            .and_then(|value| parse_dlna_rel_time(&value)),
        soap_response_text(xml, "TrackDuration")
            .ok()
            .and_then(|value| parse_dlna_rel_time(&value)),
    ))
}

fn parse_get_volume_response(xml: &str) -> Result<Option<f64>, String> {
    let value = soap_response_text(xml, "CurrentVolume")?;
    Ok(value
        .parse::<f64>()
        .ok()
        .filter(|volume| volume.is_finite())
        .map(|volume| volume.clamp(0.0, 100.0)))
}

fn soap_response_text(xml: &str, name: &str) -> Result<String, String> {
    let document = Document::parse(xml).map_err(|_| "invalid DLNA SOAP response XML".to_string())?;
    document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case(name))
        .and_then(|node| node.text())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("DLNA SOAP response has no {name}"))
}

fn parse_dlna_rel_time(value: &str) -> Option<f64> {
    let mut fields = value.trim().split(':');
    let hours = fields.next()?.parse::<f64>().ok()?;
    let minutes = fields.next()?.parse::<f64>().ok()?;
    let seconds = fields.next()?.parse::<f64>().ok()?;
    if fields.next().is_some()
        || !hours.is_finite()
        || !minutes.is_finite()
        || !seconds.is_finite()
        || hours < 0.0
        || !(0.0..60.0).contains(&minutes)
        || !(0.0..60.0).contains(&seconds)
    {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_protocol_info_list(value: &str) -> Vec<DlnaProtocolInfo> {
    value
        .split(',')
        .filter_map(|entry| {
            let mut fields = entry.trim().splitn(4, ':');
            let transport = fields.next()?.trim();
            let network = fields.next()?.trim();
            let content_format = fields.next()?.trim();
            let additional_info = fields.next()?.trim();
            if transport.is_empty() || content_format.is_empty() {
                return None;
            }
            Some(DlnaProtocolInfo {
                transport: transport.to_string(),
                network: network.to_string(),
                content_format: content_format.to_string(),
                additional_info: additional_info.to_string(),
            })
        })
        .collect()
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
        sink_protocols: None,
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
    if control_url.host_str() != description_url.host_str() {
        return Err("service control URL host does not match the renderer".to_string());
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
        build_get_position_info_envelope, build_get_protocol_info_envelope,
        build_get_transport_info_envelope, build_get_volume_envelope, build_pause_envelope,
        build_play_envelope, build_seek_envelope, build_set_av_transport_uri_envelope,
        build_set_volume_envelope, build_stop_envelope, conservative_capabilities, format_dlna_rel_time,
        location_ipv4_for_response, parse_get_position_info_response,
        parse_get_protocol_info_response, parse_get_transport_info_response,
        parse_get_volume_response, parse_renderer_description, udn_from_usn,
    };
    use crate::casting::CastMediaDescriptor;
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
    fn rejects_a_control_url_for_a_different_host() {
        let description_url = Url::parse("http://192.0.2.20:25826/description.xml").unwrap();
        let xml = r#"
            <root><device>
              <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
              <serviceList><service>
                <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
                <controlURL>http://192.0.2.21:1400/control</controlURL>
              </service></serviceList>
            </device></root>
        "#;

        assert!(parse_renderer_description(&description_url, xml).is_err());
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

    #[test]
    fn parses_sink_protocol_info_and_matches_declared_mime_types() {
        let sink_protocols = parse_get_protocol_info_response(include_str!(
            "../fixtures/dlna_renderer/get-protocol-info.xml"
        ))
        .unwrap()
        .expect("fixture declares Sink protocolInfo");

        assert_eq!(sink_protocols.entries.len(), 4);
        assert!(sink_protocols.supports_mime_type("video/mp4"));
        assert!(sink_protocols.supports_mime_type("video/x-matroska"));
        assert!(!sink_protocols.supports_mime_type("video/webm"));
    }

    #[test]
    fn treats_missing_or_empty_sink_protocol_info_as_unknown() {
        let missing_sink = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
              <s:Body><u:GetProtocolInfoResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1">
                <Source>http-get:*:video/mp4:*</Source>
              </u:GetProtocolInfoResponse></s:Body>
            </s:Envelope>
        "#;
        let empty_sink = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
              <s:Body><u:GetProtocolInfoResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1">
                <Sink>   </Sink>
              </u:GetProtocolInfoResponse></s:Body>
            </s:Envelope>
        "#;

        assert_eq!(parse_get_protocol_info_response(missing_sink).unwrap(), None);
        assert_eq!(parse_get_protocol_info_response(empty_sink).unwrap(), None);
    }

    #[test]
    fn only_http_get_sink_entries_authorize_http_media_delivery() {
        let response = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
              <s:Body><u:GetProtocolInfoResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1">
                <Sink>rtsp-rtp-udp:*:video/mp4:*</Sink>
              </u:GetProtocolInfoResponse></s:Body>
            </s:Envelope>
        "#;
        let sink_protocols = parse_get_protocol_info_response(response)
            .unwrap()
            .expect("response declares a Sink protocolInfo");

        assert!(!sink_protocols.supports_mime_type("video/mp4"));
    }

    #[test]
    fn builds_a_get_protocol_info_soap_envelope() {
        let service_type = "urn:schemas-upnp-org:service:ConnectionManager:1";
        let envelope = build_get_protocol_info_envelope(service_type);

        assert!(envelope.contains("<u:GetProtocolInfo"));
        assert!(envelope.contains(service_type));
    }

    #[test]
    fn builds_escaped_didl_lite_for_set_av_transport_uri() {
        let media = CastMediaDescriptor {
            url: "http://192.0.2.1/cast/lease/media?part=one&token=two".to_string(),
            title: Some("A <B> & C".to_string()),
            mime_type: Some("video/mp4".to_string()),
            duration: Some(42.0),
            position: 0.0,
        };
        let envelope = build_set_av_transport_uri_envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            &media,
        );
        let soap = roxmltree::Document::parse(&envelope).unwrap();
        let metadata = soap
            .descendants()
            .find(|node| node.tag_name().name() == "CurrentURIMetaData")
            .and_then(|node| node.text())
            .unwrap();
        let didl = roxmltree::Document::parse(metadata).unwrap();

        assert_eq!(
            didl.descendants()
                .find(|node| node.tag_name().name() == "title")
                .and_then(|node| node.text()),
            Some("A <B> & C"),
        );
        assert_eq!(
            didl.descendants()
                .find(|node| node.tag_name().name() == "res")
                .and_then(|node| node.text()),
            Some("http://192.0.2.1/cast/lease/media?part=one&token=two"),
        );
        assert!(envelope.contains("SetAVTransportURI"));
        assert!(envelope.contains("http-get:*:video/mp4:*"));
    }

    #[test]
    fn builds_a_play_soap_envelope() {
        let envelope = build_play_envelope("urn:schemas-upnp-org:service:AVTransport:1");

        assert!(envelope.contains("<u:Play"));
        assert!(envelope.contains("<InstanceID>0</InstanceID>"));
        assert!(envelope.contains("<Speed>1</Speed>"));
    }

    #[test]
    fn builds_pause_stop_and_relative_time_seek_envelopes() {
        let service_type = "urn:schemas-upnp-org:service:AVTransport:1";

        assert!(build_pause_envelope(service_type).contains("<u:Pause"));
        assert!(build_stop_envelope(service_type).contains("<u:Stop"));
        let target = format_dlna_rel_time(3723.9).unwrap();
        assert_eq!(target, "01:02:03");
        let seek = build_seek_envelope(service_type, &target);
        assert!(seek.contains("<Unit>REL_TIME</Unit>"));
        assert!(seek.contains("<Target>01:02:03</Target>"));
        assert!(format_dlna_rel_time(-1.0).is_err());
        assert!(format_dlna_rel_time(f64::NAN).is_err());
    }

    #[test]
    fn parses_renderer_transport_position_and_volume_status() {
        assert_eq!(
            parse_get_transport_info_response(include_str!(
                "../fixtures/dlna_renderer/get-transport-info.xml"
            ))
            .unwrap(),
            soia_protocol::CastPhaseDto::Stopped,
        );
        assert_eq!(
            parse_get_position_info_response(include_str!(
                "../fixtures/dlna_renderer/get-position-info.xml"
            ))
            .unwrap(),
            (Some(0.0), Some(0.0)),
        );
        assert_eq!(
            parse_get_volume_response(include_str!("../fixtures/dlna_renderer/get-volume.xml"))
                .unwrap(),
            Some(0.0),
        );
    }

    #[test]
    fn builds_renderer_status_soap_envelopes() {
        let av_transport = "urn:schemas-upnp-org:service:AVTransport:1";
        let rendering_control = "urn:schemas-upnp-org:service:RenderingControl:1";

        assert!(build_get_transport_info_envelope(av_transport).contains("GetTransportInfo"));
        assert!(build_get_position_info_envelope(av_transport).contains("GetPositionInfo"));
        let volume = build_get_volume_envelope(rendering_control);
        assert!(volume.contains("GetVolume"));
        assert!(volume.contains("<Channel>Master</Channel>"));
        let set_volume = build_set_volume_envelope(rendering_control, 37);
        assert!(set_volume.contains("SetVolume"));
        assert!(set_volume.contains("<DesiredVolume>37</DesiredVolume>"));
    }
}
