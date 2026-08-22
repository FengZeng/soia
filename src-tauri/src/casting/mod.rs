#![allow(dead_code)]

//! Protocol-neutral casting boundary.
//!
//! This module intentionally contains no discovery sockets, receiver transport, or AppState
//! integration yet.

pub(crate) mod source;
mod service;

pub(crate) use service::CastingService;
pub(crate) use source::CastMediaSource;

use futures_util::future::BoxFuture;
use soia_protocol::{
    CastDeviceDto, CastErrorDto, CastPhaseDto, CastProtocolDto,
};

#[derive(Clone, Debug)]
pub(crate) struct CastMediaDescriptor {
    /// Session-scoped media gateway URL. It is never sent to UI clients.
    pub url: String,
    pub title: Option<String>,
    pub mime_type: Option<String>,
    pub duration: Option<f64>,
    pub position: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CastAdapterSession {
    /// Adapter-owned remote session identity; Core will wrap it in its own cast session ID.
    pub id: String,
    pub device_id: String,
}

#[derive(Clone, Debug)]
pub(crate) struct CastReceiverStatus {
    pub phase: CastPhaseDto,
    pub position: f64,
    pub duration: Option<f64>,
    pub volume: Option<f64>,
    pub muted: Option<bool>,
    pub seekable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CastProtocolCommand {
    Play,
    Pause,
    SeekAbsolute { position: f64 },
    SeekRelative { seconds: f64 },
    Stop,
    SetVolume { volume: f64 },
    SetMuted { muted: bool },
}

/// Common receiver contract. SOAP, mDNS, TLS, protobuf, and transport identifiers stay behind
/// this trait so `CastingService` can route one set of playback commands for every protocol.
pub(crate) trait CastProtocolAdapter: Send + Sync {
    fn protocol(&self) -> CastProtocolDto;

    fn discover<'a>(&'a self) -> BoxFuture<'a, Result<Vec<CastDeviceDto>, CastErrorDto>>;

    fn connect<'a>(
        &'a self,
        device: &'a CastDeviceDto,
    ) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>>;

    fn load<'a>(
        &'a self,
        session: &'a CastAdapterSession,
        media: &'a CastMediaDescriptor,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>>;

    fn command<'a>(
        &'a self,
        session: &'a CastAdapterSession,
        command: CastProtocolCommand,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>>;

    fn status<'a>(
        &'a self,
        session: &'a CastAdapterSession,
    ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>>;

    fn disconnect<'a>(
        &'a self,
        session: &'a CastAdapterSession,
    ) -> BoxFuture<'a, Result<(), CastErrorDto>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use soia_protocol::{CastCapabilitiesDto, CastErrorCodeDto};

    struct FakeAdapter {
        protocol: CastProtocolDto,
        device_id: &'static str,
    }

    impl FakeAdapter {
        fn status() -> CastReceiverStatus {
            CastReceiverStatus {
                phase: CastPhaseDto::Playing,
                position: 12.0,
                duration: Some(60.0),
                volume: Some(35.0),
                muted: Some(false),
                seekable: true,
            }
        }
    }

    impl CastProtocolAdapter for FakeAdapter {
        fn protocol(&self) -> CastProtocolDto { self.protocol.clone() }

        fn discover<'a>(&'a self) -> BoxFuture<'a, Result<Vec<CastDeviceDto>, CastErrorDto>> {
            Box::pin(async move {
                Ok(vec![CastDeviceDto {
                    id: self.device_id.to_string(),
                    protocol: self.protocol(),
                    name: "Test receiver".to_string(),
                    model_name: None,
                    address: "192.0.2.10".to_string(),
                    capabilities: CastCapabilitiesDto {
                        play: true, pause: true, seek: true, stop: true, volume: true,
                    },
                    last_seen_at: 1,
                }])
            })
        }

        fn connect<'a>(&'a self, device: &'a CastDeviceDto) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>> {
            Box::pin(async move { Ok(CastAdapterSession { id: "remote-session".to_string(), device_id: device.id.clone() }) })
        }

        fn load<'a>(&'a self, _session: &'a CastAdapterSession, _media: &'a CastMediaDescriptor) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
            Box::pin(async move { Ok(Self::status()) })
        }

        fn command<'a>(&'a self, _session: &'a CastAdapterSession, _command: CastProtocolCommand) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
            Box::pin(async move { Ok(Self::status()) })
        }

        fn status<'a>(&'a self, _session: &'a CastAdapterSession) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
            Box::pin(async move { Ok(Self::status()) })
        }

        fn disconnect<'a>(&'a self, _session: &'a CastAdapterSession) -> BoxFuture<'a, Result<(), CastErrorDto>> {
            Box::pin(async move { Ok(()) })
        }
    }

    #[test]
    fn fake_adapters_cover_two_protocols_without_protocol_leaks() {
        let adapters: Vec<Box<dyn CastProtocolAdapter>> = vec![
            Box::new(FakeAdapter { protocol: CastProtocolDto::Dlna, device_id: "uuid:dlna-1" }),
            Box::new(FakeAdapter { protocol: CastProtocolDto::Chromecast, device_id: "cast-1" }),
        ];
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let discovered = adapters
            .iter()
            .flat_map(|adapter| runtime.block_on(adapter.discover()).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].protocol, CastProtocolDto::Dlna);
        assert_eq!(discovered[1].protocol, CastProtocolDto::Chromecast);
        assert!(discovered.iter().all(|device| device.capabilities.seek));

        let media = CastMediaDescriptor {
            url: "http://192.0.2.1:1234/cast/lease/media".to_string(),
            title: Some("Test video".to_string()),
            mime_type: Some("video/mp4".to_string()),
            duration: Some(60.0),
            position: 12.0,
        };
        let session = runtime.block_on(adapters[0].connect(&discovered[0])).unwrap();
        let loaded = runtime.block_on(adapters[0].load(&session, &media)).unwrap();
        let paused = runtime
            .block_on(adapters[0].command(&session, CastProtocolCommand::Pause))
            .unwrap();
        let current = runtime.block_on(adapters[0].status(&session)).unwrap();
        runtime.block_on(adapters[0].disconnect(&session)).unwrap();

        assert_eq!(session.device_id, "uuid:dlna-1");
        assert_eq!(loaded.phase, CastPhaseDto::Playing);
        assert_eq!(paused.position, 12.0);
        assert_eq!(current.duration, Some(60.0));
        assert_eq!(current.volume, Some(35.0));
        assert_eq!(current.muted, Some(false));
        assert!(current.seekable);

        let error = CastErrorDto {
            code: CastErrorCodeDto::LoadFailed,
            message: "receiver rejected the media".to_string(),
            device_id: Some(discovered[0].id.clone()),
        };
        assert_eq!(error.device_id.as_deref(), Some("uuid:dlna-1"));
    }

    #[test]
    fn real_renderer_fixture_is_redacted_and_has_required_dlna_services() {
        let ssdp = include_str!("fixtures/dlna_renderer/ssdp-response.txt");
        assert!(ssdp.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1"));
        assert!(ssdp.contains("LOCATION: http://192.0.2.20:25826/description.xml"));
        assert!(!ssdp.contains("192.168."));

        let description = roxmltree::Document::parse(include_str!("fixtures/dlna_renderer/description.xml")).unwrap();
        let service_types = description
            .descendants()
            .filter(|node| node.has_tag_name("serviceType"))
            .filter_map(|node| node.text())
            .collect::<Vec<_>>();
        assert!(service_types.contains(&"urn:schemas-upnp-org:service:ConnectionManager:1"));
        assert!(service_types.contains(&"urn:schemas-upnp-org:service:AVTransport:1"));
        assert!(service_types.contains(&"urn:schemas-upnp-org:service:RenderingControl:1"));

        let protocol_info = include_str!("fixtures/dlna_renderer/get-protocol-info.xml");
        assert!(protocol_info.contains("video/mp4"));
        assert!(protocol_info.contains("video/mkv"));

        let transport_info = include_str!("fixtures/dlna_renderer/get-transport-info.xml");
        let position_info = include_str!("fixtures/dlna_renderer/get-position-info.xml");
        assert!(transport_info.contains("<CurrentTransportState>STOPPED</CurrentTransportState>"));
        assert!(position_info.contains("https://redacted.invalid/media"));
        assert!(!position_info.contains("192.168."));
    }
}
