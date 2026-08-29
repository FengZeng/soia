use super::{CastAdapterSession, CastMediaDescriptor, CastProtocolAdapter, CastProtocolCommand, CastReceiverStatus};
use futures_util::future::BoxFuture;
use soia_protocol::{CastCapabilitiesDto, CastDeviceDto, CastErrorDto, CastPhaseDto, CastProtocolDto};
use std::sync::Arc;

struct FixtureAdapter {
    protocol: CastProtocolDto,
    device_id: &'static str,
    name: &'static str,
}

impl FixtureAdapter {
    fn device(&self) -> CastDeviceDto {
        CastDeviceDto {
            id: self.device_id.to_string(),
            protocol: self.protocol.clone(),
            name: self.name.to_string(),
            model_name: Some("Development fixture".to_string()),
            address: "192.0.2.20".to_string(),
            capabilities: CastCapabilitiesDto {
                play: true,
                pause: true,
                seek: true,
                stop: true,
                volume: true,
            },
            last_seen_at: 0,
        }
    }
}

impl CastProtocolAdapter for FixtureAdapter {
    fn protocol(&self) -> CastProtocolDto { self.protocol.clone() }

    fn discover<'a>(&'a self) -> BoxFuture<'a, Result<Vec<CastDeviceDto>, CastErrorDto>> {
        Box::pin(async move { Ok(vec![self.device()]) })
    }

    fn connect<'a>(&'a self, device: &'a CastDeviceDto) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>> {
        Box::pin(async move {
            Ok(CastAdapterSession {
                id: format!("fixture-session-{}", device.id),
                device_id: device.id.clone(),
            })
        })
    }

    fn load<'a>(&'a self, _session: &'a CastAdapterSession, media: &'a CastMediaDescriptor) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            Ok(CastReceiverStatus {
                phase: CastPhaseDto::Playing,
                position: media.position,
                duration: media.duration,
                volume: Some(25.0),
                muted: Some(false),
                seekable: true,
                ended_naturally: false,
            })
        })
    }

    fn command<'a>(&'a self, _session: &'a CastAdapterSession, command: CastProtocolCommand) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let phase = match command {
                CastProtocolCommand::Pause => CastPhaseDto::Paused,
                CastProtocolCommand::Stop => CastPhaseDto::Stopped,
                _ => CastPhaseDto::Playing,
            };
            Ok(CastReceiverStatus {
                phase,
                position: 0.0,
                duration: None,
                volume: Some(25.0),
                muted: Some(false),
                seekable: true,
                ended_naturally: false,
            })
        })
    }

    fn status<'a>(&'a self, _session: &'a CastAdapterSession) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async {
            Ok(CastReceiverStatus {
                phase: CastPhaseDto::Playing,
                position: 0.0,
                duration: None,
                volume: Some(25.0),
                muted: Some(false),
                seekable: true,
                ended_naturally: false,
            })
        })
    }

    fn disconnect<'a>(&'a self, _session: &'a CastAdapterSession) -> BoxFuture<'a, Result<(), CastErrorDto>> {
        Box::pin(async { Ok(()) })
    }
}

pub(crate) fn adapters() -> Vec<Arc<dyn CastProtocolAdapter>> {
    vec![
        Arc::new(FixtureAdapter {
            protocol: CastProtocolDto::Dlna,
            device_id: "fixture-dlna-renderer",
            name: "DLNA Renderer (fixture)",
        }),
        Arc::new(FixtureAdapter {
            protocol: CastProtocolDto::Chromecast,
            device_id: "fixture-chromecast",
            name: "Chromecast (fixture)",
        }),
    ]
}
