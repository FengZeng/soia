use super::{CastAdapterSession, CastProtocolAdapter, CastReceiverStatus};
use soia_protocol::{CastDeviceDto, CastErrorDto, CastPhaseDto, CastSnapshotDto};
use std::sync::Arc;

pub(super) struct ActiveCastSession {
    pub(super) session_id: String,
    pub(super) device: CastDeviceDto,
    pub(super) adapter: Arc<dyn CastProtocolAdapter>,
    pub(super) adapter_session: Option<CastAdapterSession>,
    pub(super) media_title: Option<String>,
    pub(super) status: CastReceiverStatus,
}

pub(super) struct CastingState {
    pub(super) revision: u64,
    pub(super) devices: Vec<CastDeviceDto>,
    pub(super) phase: CastPhaseDto,
    pub(super) active: Option<ActiveCastSession>,
    pub(super) last_error: Option<CastErrorDto>,
}

impl Default for CastingState {
    fn default() -> Self {
        Self {
            revision: 0,
            devices: Vec::new(),
            phase: CastPhaseDto::Idle,
            active: None,
            last_error: None,
        }
    }
}

impl CastingState {
    pub(super) fn snapshot(&self) -> CastSnapshotDto {
        let Some(active) = self.active.as_ref() else {
            return CastSnapshotDto {
                revision: self.revision,
                phase: self.phase.clone(),
                last_error: self.last_error.clone(),
                ..Default::default()
            };
        };
        CastSnapshotDto {
            revision: self.revision,
            phase: self.phase.clone(),
            session_id: Some(active.session_id.clone()),
            device: Some(active.device.clone()),
            media_title: active.media_title.clone(),
            position: active.status.position,
            duration: active.status.duration.unwrap_or(0.0),
            volume: active.status.volume.unwrap_or(100.0),
            muted: active.status.muted.unwrap_or(false),
            seekable: active.status.seekable,
            last_error: self.last_error.clone(),
        }
    }
}
