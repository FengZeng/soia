use super::{CastAdapterSession, CastMediaDescriptor, CastProtocolAdapter, CastProtocolCommand, CastReceiverStatus};
use futures_util::future::join_all;
use soia_protocol::{CastDeviceDto, CastErrorCodeDto, CastErrorDto, CastPhaseDto, CastSnapshotDto};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

struct ActiveCastSession {
    session_id: String,
    device: CastDeviceDto,
    adapter: Arc<dyn CastProtocolAdapter>,
    adapter_session: Option<CastAdapterSession>,
    media_title: Option<String>,
    status: CastReceiverStatus,
}

struct CastingState {
    revision: u64,
    devices: Vec<CastDeviceDto>,
    phase: CastPhaseDto,
    active: Option<ActiveCastSession>,
    last_error: Option<CastErrorDto>,
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
    fn snapshot(&self) -> CastSnapshotDto {
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

/// Protocol-neutral Core owner for receiver discovery and one active cast session. Every async
/// completion is checked against the Core session ID so a previous device cannot overwrite a
/// newer selection.
pub(crate) struct CastingService {
    adapters: Vec<Arc<dyn CastProtocolAdapter>>,
    state: Mutex<CastingState>,
    snapshot_sender: watch::Sender<CastSnapshotDto>,
}

impl CastingService {
    pub(crate) fn new(adapters: Vec<Arc<dyn CastProtocolAdapter>>) -> Self {
        let (snapshot_sender, _) = watch::channel(CastSnapshotDto::default());
        Self {
            adapters,
            state: Mutex::new(CastingState::default()),
            snapshot_sender,
        }
    }

    pub(crate) fn current_snapshot(&self) -> CastSnapshotDto {
        self.snapshot_sender.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<CastSnapshotDto> {
        self.snapshot_sender.subscribe()
    }

    pub(crate) fn devices(&self) -> Vec<CastDeviceDto> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .devices
            .clone()
    }

    pub(crate) async fn discover(&self) -> Result<Vec<CastDeviceDto>, CastErrorDto> {
        let has_active_session = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active
            .is_some();
        if !has_active_session {
            self.publish(|state| {
                state.phase = CastPhaseDto::Discovering;
                state.last_error = None;
            });
        }

        let results = join_all(self.adapters.iter().map(|adapter| adapter.discover())).await;
        let mut devices = Vec::new();
        let mut first_error = None;
        for result in results {
            match result {
                Ok(mut found) => devices.append(&mut found),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        devices.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        devices.dedup_by(|left, right| left.protocol == right.protocol && left.id == right.id);
        let result = devices.clone();
        self.publish(|state| {
            state.devices = devices;
            if state.active.is_none() {
                state.phase = first_error
                    .as_ref()
                    .map(|_| CastPhaseDto::Error)
                    .unwrap_or(CastPhaseDto::Idle);
                state.last_error = first_error;
            }
        });
        if result.is_empty() {
            if let Some(error) = self.current_snapshot().last_error {
                return Err(error);
            }
        }
        Ok(result)
    }

    pub(crate) async fn connect_and_load(
        &self,
        device_id: &str,
        media: CastMediaDescriptor,
    ) -> Result<CastSnapshotDto, CastErrorDto> {
        self.connect_and_load_with(device_id, move |_, _| Ok(media)).await
    }

    pub(crate) async fn connect_and_load_with<F>(
        &self,
        device_id: &str,
        create_media: F,
    ) -> Result<CastSnapshotDto, CastErrorDto>
    where
        F: FnOnce(&str, &CastDeviceDto) -> Result<CastMediaDescriptor, CastErrorDto>,
    {
        let (device, adapter) = self.device_and_adapter(device_id)?;
        let session_id = uuid::Uuid::new_v4().to_string();
        self.publish(|state| {
            state.phase = CastPhaseDto::Connecting;
            state.last_error = None;
            state.active = Some(ActiveCastSession {
                session_id: session_id.clone(),
                device: device.clone(),
                adapter: adapter.clone(),
                adapter_session: None,
                media_title: None,
                status: empty_status(CastPhaseDto::Connecting),
            });
        });

        let adapter_session = match adapter.connect(&device).await {
            Ok(session) => session,
            Err(error) => return Err(self.fail_session(&session_id, error)),
        };
        if !self.install_adapter_session(&session_id, adapter_session.clone()) {
            let _ = adapter.disconnect(&adapter_session).await;
            return Err(stale_session_error(&device.id));
        }
        let media = match create_media(&session_id, &device) {
            Ok(media) => media,
            Err(error) => {
                let failure = self.fail_session(&session_id, error);
                let _ = adapter.disconnect(&adapter_session).await;
                return Err(failure);
            }
        };
        self.publish_if_current(&session_id, |state, active| {
            state.phase = CastPhaseDto::Loading;
            active.status = empty_status(CastPhaseDto::Loading);
            active.media_title = media.title.clone();
        });

        let status = match adapter.load(&adapter_session, &media).await {
            Ok(status) => status,
            Err(error) => {
                let failure = self.fail_session(&session_id, error);
                let _ = adapter.disconnect(&adapter_session).await;
                return Err(failure);
            }
        };
        if !self.apply_status(&session_id, status) {
            let _ = adapter.disconnect(&adapter_session).await;
            return Err(stale_session_error(&device.id));
        }
        Ok(self.current_snapshot())
    }

    pub(crate) async fn command(
        &self,
        command: CastProtocolCommand,
    ) -> Result<CastSnapshotDto, CastErrorDto> {
        let (session_id, device_id, adapter, adapter_session) = self.active_adapter_session()?;
        let status = match adapter.command(&adapter_session, command).await {
            Ok(status) => status,
            Err(error) => return Err(self.fail_session(&session_id, error)),
        };
        if !self.apply_status(&session_id, status) {
            return Err(stale_session_error(&device_id));
        }
        Ok(self.current_snapshot())
    }

    pub(crate) async fn refresh_status(&self) -> Result<CastSnapshotDto, CastErrorDto> {
        let (session_id, device_id, adapter, adapter_session) = self.active_adapter_session()?;
        let status = match adapter.status(&adapter_session).await {
            Ok(status) => status,
            Err(error) => return Err(self.fail_session(&session_id, error)),
        };
        if !self.apply_status(&session_id, status) {
            return Err(stale_session_error(&device_id));
        }
        Ok(self.current_snapshot())
    }

    pub(crate) async fn disconnect(&self) -> Result<CastSnapshotDto, CastErrorDto> {
        let active = {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let active = state.active.take();
            if active.is_some() {
                state.phase = CastPhaseDto::Idle;
                state.last_error = None;
                state.revision = state.revision.saturating_add(1);
                self.snapshot_sender.send_replace(state.snapshot());
            }
            active
        };
        let Some(active) = active else {
            return Ok(self.current_snapshot());
        };
        crate::media_gateway::revoke_cast_media_session(&active.session_id);
        if let Some(adapter_session) = active.adapter_session {
            active.adapter.disconnect(&adapter_session).await?;
        }
        Ok(self.current_snapshot())
    }

    fn device_and_adapter(
        &self,
        device_id: &str,
    ) -> Result<(CastDeviceDto, Arc<dyn CastProtocolAdapter>), CastErrorDto> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let device = state
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .cloned()
            .ok_or_else(|| error(CastErrorCodeDto::DeviceUnsupported, "cast device is unavailable", Some(device_id)))?;
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.protocol() == device.protocol)
            .cloned()
            .ok_or_else(|| error(CastErrorCodeDto::DeviceUnsupported, "cast protocol is unavailable", Some(device_id)))?;
        Ok((device, adapter))
    }

    fn active_adapter_session(
        &self,
    ) -> Result<(String, String, Arc<dyn CastProtocolAdapter>, CastAdapterSession), CastErrorDto> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let active = state.active.as_ref().ok_or_else(|| {
            error(CastErrorCodeDto::CommandFailed, "no active cast session", None)
        })?;
        let adapter_session = active.adapter_session.clone().ok_or_else(|| {
            error(CastErrorCodeDto::CommandFailed, "cast session is not connected", Some(&active.device.id))
        })?;
        Ok((
            active.session_id.clone(),
            active.device.id.clone(),
            active.adapter.clone(),
            adapter_session,
        ))
    }

    fn install_adapter_session(&self, session_id: &str, adapter_session: CastAdapterSession) -> bool {
        self.publish_if_current(session_id, |_, active| active.adapter_session = Some(adapter_session))
    }

    fn apply_status(&self, session_id: &str, status: CastReceiverStatus) -> bool {
        self.publish_if_current(session_id, |state, active| {
            state.phase = status.phase.clone();
            active.status = status;
        })
    }

    fn fail_session(&self, session_id: &str, failure: CastErrorDto) -> CastErrorDto {
        if self.publish_if_current(session_id, |state, _| {
            state.phase = CastPhaseDto::Error;
            state.last_error = Some(failure.clone());
        }) {
            crate::media_gateway::revoke_cast_media_session(session_id);
        }
        failure
    }

    fn publish_if_current(
        &self,
        session_id: &str,
        update: impl FnOnce(&mut CastingState, &mut ActiveCastSession),
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(mut active) = state.active.take() else {
            return false;
        };
        if active.session_id != session_id {
            state.active = Some(active);
            return false;
        }
        update(&mut state, &mut active);
        state.active = Some(active);
        state.revision = state.revision.saturating_add(1);
        self.snapshot_sender.send_replace(state.snapshot());
        true
    }

    fn publish(&self, update: impl FnOnce(&mut CastingState)) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut state);
        state.revision = state.revision.saturating_add(1);
        self.snapshot_sender.send_replace(state.snapshot());
    }
}

fn empty_status(phase: CastPhaseDto) -> CastReceiverStatus {
    CastReceiverStatus {
        phase,
        position: 0.0,
        duration: None,
        volume: None,
        muted: None,
        seekable: false,
    }
}

fn error(code: CastErrorCodeDto, message: &str, device_id: Option<&str>) -> CastErrorDto {
    CastErrorDto {
        code,
        message: message.to_string(),
        device_id: device_id.map(str::to_string),
    }
}

fn stale_session_error(device_id: &str) -> CastErrorDto {
    error(
        CastErrorCodeDto::CommandFailed,
        "cast session was replaced by a newer selection",
        Some(device_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::BoxFuture;
    use soia_protocol::{CastCapabilitiesDto, CastProtocolDto};

    struct FakeAdapter;

    fn device(id: &str) -> CastDeviceDto {
        CastDeviceDto {
            id: id.to_string(),
            protocol: CastProtocolDto::Dlna,
            name: format!("Receiver {id}"),
            model_name: None,
            address: "192.0.2.20".to_string(),
            capabilities: CastCapabilitiesDto {
                play: true,
                pause: true,
                seek: true,
                stop: true,
                volume: true,
            },
            last_seen_at: 1,
        }
    }

    fn receiver_status(phase: CastPhaseDto, position: f64) -> CastReceiverStatus {
        CastReceiverStatus {
            phase,
            position,
            duration: Some(60.0),
            volume: Some(25.0),
            muted: Some(false),
            seekable: true,
        }
    }

    impl CastProtocolAdapter for FakeAdapter {
        fn protocol(&self) -> soia_protocol::CastProtocolDto { CastProtocolDto::Dlna }

        fn discover<'a>(&'a self) -> BoxFuture<'a, Result<Vec<CastDeviceDto>, CastErrorDto>> {
            Box::pin(async { Ok(vec![device("device-a"), device("device-b")]) })
        }

        fn connect<'a>(
            &'a self,
            device: &'a CastDeviceDto,
        ) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>> {
            Box::pin(async move {
                Ok(CastAdapterSession {
                    id: format!("remote-{}", device.id),
                    device_id: device.id.clone(),
                })
            })
        }

        fn load<'a>(
            &'a self,
            _session: &'a CastAdapterSession,
            _media: &'a CastMediaDescriptor,
        ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
            Box::pin(async { Ok(receiver_status(CastPhaseDto::Playing, 12.0)) })
        }

        fn command<'a>(
            &'a self,
            _session: &'a CastAdapterSession,
            command: CastProtocolCommand,
        ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
            Box::pin(async move {
                let phase = match command {
                    CastProtocolCommand::Pause => CastPhaseDto::Paused,
                    _ => CastPhaseDto::Playing,
                };
                Ok(receiver_status(phase, 18.0))
            })
        }

        fn status<'a>(
            &'a self,
            _session: &'a CastAdapterSession,
        ) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
            Box::pin(async { Ok(receiver_status(CastPhaseDto::Playing, 20.0)) })
        }

        fn disconnect<'a>(
            &'a self,
            _session: &'a CastAdapterSession,
        ) -> BoxFuture<'a, Result<(), CastErrorDto>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn media() -> CastMediaDescriptor {
        CastMediaDescriptor {
            url: "http://192.0.2.1:39002/cast/test/media".to_string(),
            title: Some("Test media".to_string()),
            mime_type: Some("video/mp4".to_string()),
            duration: Some(60.0),
            position: 0.0,
        }
    }

    #[tokio::test]
    async fn fake_adapter_drives_discovery_session_and_controls() {
        let service = CastingService::new(vec![Arc::new(FakeAdapter)]);
        assert_eq!(service.discover().await.unwrap().len(), 2);
        let loaded = service.connect_and_load("device-a", media()).await.unwrap();
        assert_eq!(loaded.phase, CastPhaseDto::Playing);
        assert_eq!(loaded.device.unwrap().id, "device-a");
        assert_eq!(loaded.position, 12.0);

        let paused = service.command(CastProtocolCommand::Pause).await.unwrap();
        assert_eq!(paused.phase, CastPhaseDto::Paused);
        assert_eq!(paused.position, 18.0);
        assert!(paused.revision > loaded.revision);

        assert_eq!(service.disconnect().await.unwrap().phase, CastPhaseDto::Idle);
    }

    #[tokio::test]
    async fn previous_session_status_is_dropped_after_device_switch() {
        let service = CastingService::new(vec![Arc::new(FakeAdapter)]);
        service.discover().await.unwrap();
        let first = service.connect_and_load("device-a", media()).await.unwrap();
        let first_session_id = first.session_id.unwrap();
        let second = service.connect_and_load("device-b", media()).await.unwrap();

        assert!(!service.apply_status(&first_session_id, receiver_status(CastPhaseDto::Paused, 1.0)));
        let current = service.current_snapshot();
        assert_eq!(current.session_id, second.session_id);
        assert_eq!(current.device.unwrap().id, "device-b");
        assert_eq!(current.phase, CastPhaseDto::Playing);
    }
}
