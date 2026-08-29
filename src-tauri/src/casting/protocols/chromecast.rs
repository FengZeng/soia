use super::super::{
    CastAdapterSession, CastMediaDescriptor, CastProtocolAdapter, CastProtocolCommand,
    CastReceiverStatus,
};
use futures_util::future::BoxFuture;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use serde_json::{json, Value};
use soia_protocol::{
    CastCapabilitiesDto, CastDeviceDto, CastErrorCodeDto, CastErrorDto, CastPhaseDto,
    CastProtocolDto,
};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time;
use tokio_rustls::{client::TlsStream, TlsConnector};

const CAST_SERVICE: &str = "_googlecast._tcp.local";
const MDNS_MULTICAST: &str = "224.0.0.251:5353";
const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const CAST_PORT: u16 = 8009;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CAST_FRAME: usize = 1024 * 1024;
const CAST_V2_PROTOCOL_VERSION: u64 = 0;
const CAST_STRING_PAYLOAD_TYPE: u64 = 0;
const RECEIVER_ID: &str = "receiver-0";
const SENDER_ID: &str = "sender-0";
const DEFAULT_MEDIA_RECEIVER_APP_ID: &str = "CC1AD845";
const CONNECTION_NAMESPACE: &str = "urn:x-cast:com.google.cast.tp.connection";
const HEARTBEAT_NAMESPACE: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const RECEIVER_NAMESPACE: &str = "urn:x-cast:com.google.cast.receiver";
const MEDIA_NAMESPACE: &str = "urn:x-cast:com.google.cast.media";

#[derive(Clone)]
struct ChromecastDevice {
    address: String,
    port: u16,
}

pub(crate) struct ChromecastAdapter {
    devices: Mutex<HashMap<String, ChromecastDevice>>,
    sessions: AsyncMutex<HashMap<String, Arc<AsyncMutex<CastConnection>>>>,
}

impl ChromecastAdapter {
    pub(crate) fn new() -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            sessions: AsyncMutex::new(HashMap::new()),
        }
    }

    async fn session(&self, session: &CastAdapterSession) -> Result<Arc<AsyncMutex<CastConnection>>, CastErrorDto> {
        self.sessions
            .lock()
            .await
            .get(&session.id)
            .cloned()
            .ok_or_else(|| device_error(
                CastErrorCodeDto::DeviceDisconnected,
                "Chromecast connection is no longer available",
                Some(&session.device_id),
            ))
    }
}

impl CastProtocolAdapter for ChromecastAdapter {
    fn protocol(&self) -> CastProtocolDto { CastProtocolDto::Chromecast }

    fn discover<'a>(&'a self) -> BoxFuture<'a, Result<Vec<CastDeviceDto>, CastErrorDto>> {
        Box::pin(async move {
            let found = discover_chromecasts().await.map_err(|error| {
                log::debug!("Chromecast mDNS discovery failed: {error}");
                device_error(CastErrorCodeDto::DiscoveryFailed, "Chromecast discovery failed", None)
            })?;
            let mut devices = Vec::with_capacity(found.len());
            let mut cached = HashMap::new();
            for discovered in found {
                cached.insert(
                    discovered.id.clone(),
                    ChromecastDevice { address: discovered.address.clone(), port: discovered.port },
                );
                devices.push(CastDeviceDto {
                    id: discovered.id,
                    protocol: CastProtocolDto::Chromecast,
                    name: discovered.name,
                    model_name: discovered.model_name,
                    address: discovered.address,
                    capabilities: CastCapabilitiesDto { play: true, pause: true, seek: true, stop: true, volume: true },
                    last_seen_at: unix_time_secs(),
                });
            }
            *self.devices.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = cached;
            Ok(devices)
        })
    }

    fn connect<'a>(&'a self, device: &'a CastDeviceDto) -> BoxFuture<'a, Result<CastAdapterSession, CastErrorDto>> {
        Box::pin(async move {
            let endpoint = self
                .devices
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&device.id)
                .cloned()
                .ok_or_else(|| device_error(
                    CastErrorCodeDto::ConnectionFailed,
                    "Chromecast discovery data is no longer available; scan again",
                    Some(&device.id),
                ))?;
            let connection = CastConnection::connect(&endpoint.address, endpoint.port)
                .await
                .map_err(|error| {
                    log::warn!("Chromecast TLS connection failed for {}: {error}", device.id);
                    device_error(CastErrorCodeDto::ConnectionFailed, "Could not connect to Chromecast", Some(&device.id))
                })?;
            log::info!("Chromecast Cast V2 TLS connection established for {}", device.id);
            let session = CastAdapterSession { id: uuid::Uuid::new_v4().to_string(), device_id: device.id.clone() };
            self.sessions.lock().await.insert(session.id.clone(), Arc::new(AsyncMutex::new(connection)));
            Ok(session)
        })
    }

    fn load<'a>(&'a self, session: &'a CastAdapterSession, media: &'a CastMediaDescriptor) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let connection = self.session(session).await?;
            let mut connection = connection.lock().await;
            connection.load(media).await.map_err(|error| cast_error(CastErrorCodeDto::LoadFailed, &error, &session.device_id))
        })
    }

    fn command<'a>(&'a self, session: &'a CastAdapterSession, command: CastProtocolCommand) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let connection = self.session(session).await?;
            let mut connection = connection.lock().await;
            connection.command(command).await.map_err(|error| cast_error(CastErrorCodeDto::CommandFailed, &error, &session.device_id))
        })
    }

    fn status<'a>(&'a self, session: &'a CastAdapterSession) -> BoxFuture<'a, Result<CastReceiverStatus, CastErrorDto>> {
        Box::pin(async move {
            let connection = self.session(session).await?;
            let mut connection = connection.lock().await;
            connection.status().await.map_err(|error| cast_error(CastErrorCodeDto::DeviceDisconnected, &error, &session.device_id))
        })
    }

    fn disconnect<'a>(&'a self, session: &'a CastAdapterSession) -> BoxFuture<'a, Result<(), CastErrorDto>> {
        Box::pin(async move {
            if let Some(connection) = self.sessions.lock().await.remove(&session.id) {
                let mut connection = connection.lock().await;
                let _ = connection.close().await;
            }
            Ok(())
        })
    }
}

struct CastConnection {
    stream: TlsStream<TcpStream>,
    request_id: u64,
    transport_id: Option<String>,
    media_session_id: Option<u64>,
    last_status: CastReceiverStatus,
}

impl CastConnection {
    async fn connect(address: &str, port: u16) -> Result<Self, String> {
        let stream = time::timeout(CONNECTION_TIMEOUT, TcpStream::connect((address, port)))
            .await
            .map_err(|_| "connection timed out".to_string())?
            .map_err(|_| "TCP connection failed".to_string())?;
        stream.set_nodelay(true).map_err(|_| "could not configure connection".to_string())?;
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(ChromecastCertificateVerifier))
            .with_no_client_auth();
        let server_name = ServerName::try_from("chromecast.local")
            .map_err(|_| "invalid Chromecast TLS server name".to_string())?;
        let stream = time::timeout(CONNECTION_TIMEOUT, TlsConnector::from(Arc::new(config)).connect(server_name, stream))
            .await
            .map_err(|_| "TLS handshake timed out".to_string())?
            .map_err(|_| "TLS handshake failed".to_string())?;
        Ok(Self {
            stream,
            request_id: 0,
            transport_id: None,
            media_session_id: None,
            last_status: empty_status(CastPhaseDto::Connecting),
        })
    }

    async fn load(&mut self, media: &CastMediaDescriptor) -> Result<CastReceiverStatus, String> {
        log::info!("Chromecast: querying receiver status");
        self.connect_channel(RECEIVER_ID).await?;
        let receiver = self.receiver_request(json!({"type": "GET_STATUS"})).await?;
        let transport_id = default_receiver_transport_id(&receiver);
        let transport_id = match transport_id {
            Some(transport_id) => {
                log::info!("Chromecast: reusing Default Media Receiver");
                transport_id
            }
            None => {
                log::info!("Chromecast: launching Default Media Receiver");
                let launched = self.receiver_request(json!({"type": "LAUNCH", "appId": DEFAULT_MEDIA_RECEIVER_APP_ID})).await?;
                if let Some(transport_id) = default_receiver_transport_id(&launched) {
                    transport_id
                } else {
                    let status = self.receiver_request(json!({"type": "GET_STATUS"})).await?;
                    default_receiver_transport_id(&status).ok_or_else(|| "Chromecast did not start Default Media Receiver".to_string())?
                }
            }
        };
        self.transport_id = Some(transport_id.clone());
        self.connect_channel(&transport_id).await?;
        log::info!("Chromecast: sending media LOAD request");
        let content_type = media.mime_type.as_deref().unwrap_or("application/octet-stream");
        let mut request = json!({
            "type": "LOAD",
            "autoplay": true,
            "currentTime": media.position.max(0.0),
            "media": {
                "contentId": media.url,
                "contentType": content_type,
                "streamType": "BUFFERED",
                "metadata": { "metadataType": 0 }
            }
        });
        if let Some(title) = media.title.as_deref() {
            request["media"]["metadata"]["title"] = Value::String(title.to_string());
        }
        if let Some(duration) = media.duration.filter(|duration| duration.is_finite() && *duration > 0.0) {
            request["media"]["duration"] = json!(duration);
        }
        self.media_request(request).await
    }

    async fn command(&mut self, command: CastProtocolCommand) -> Result<CastReceiverStatus, String> {
        match command {
            CastProtocolCommand::SetVolume { volume } => {
                if !volume.is_finite() || !(0.0..=100.0).contains(&volume) {
                    return Err("volume must be between 0 and 100".to_string());
                }
                let receiver = self.receiver_request(json!({"type": "SET_VOLUME", "volume": {"level": volume / 100.0}})).await?;
                self.apply_receiver_volume(&receiver);
                Ok(self.last_status.clone())
            }
            CastProtocolCommand::SetMuted { muted } => {
                let receiver = self.receiver_request(json!({"type": "SET_VOLUME", "volume": {"muted": muted}})).await?;
                self.apply_receiver_volume(&receiver);
                Ok(self.last_status.clone())
            }
            CastProtocolCommand::Play => self.media_request(media_command("PLAY", self.media_session_id, None)).await,
            CastProtocolCommand::Pause => self.media_request(media_command("PAUSE", self.media_session_id, None)).await,
            CastProtocolCommand::Stop => self.media_request(media_command("STOP", self.media_session_id, None)).await,
            CastProtocolCommand::SeekAbsolute { position } => {
                if !position.is_finite() || position < 0.0 { return Err("seek position must be non-negative".to_string()); }
                self.media_request(media_command("SEEK", self.media_session_id, Some(position))).await
            }
            CastProtocolCommand::SeekRelative { seconds } => {
                if !seconds.is_finite() || !(-600.0..=600.0).contains(&seconds) { return Err("relative seek must be within 600 seconds".to_string()); }
                self.media_request(media_command("SEEK", self.media_session_id, Some((self.last_status.position + seconds).max(0.0)))).await
            }
        }
    }

    async fn status(&mut self) -> Result<CastReceiverStatus, String> {
        if self.transport_id.is_none() { return Err("Chromecast receiver session ended".to_string()); }
        self.media_request(json!({"type": "GET_STATUS"})).await
    }

    async fn close(&mut self) -> Result<(), String> {
        if let Some(transport_id) = self.transport_id.clone() {
            let _ = self.send_json(&transport_id, CONNECTION_NAMESPACE, json!({"type": "CLOSE"})).await;
        }
        let _ = self.send_json(RECEIVER_ID, CONNECTION_NAMESPACE, json!({"type": "CLOSE"})).await;
        self.stream.shutdown().await.map_err(|_| "could not close Chromecast connection".to_string())
    }

    async fn connect_channel(&mut self, destination_id: &str) -> Result<(), String> {
        self.send_json(destination_id, CONNECTION_NAMESPACE, json!({"type": "CONNECT"})).await
    }

    async fn receiver_request(&mut self, mut payload: Value) -> Result<Value, String> {
        let request_id = self.next_request_id();
        payload["requestId"] = json!(request_id);
        self.send_json(RECEIVER_ID, RECEIVER_NAMESPACE, payload).await?;
        self.wait_for_response(request_id, RECEIVER_NAMESPACE).await
    }

    async fn media_request(&mut self, mut payload: Value) -> Result<CastReceiverStatus, String> {
        let transport_id = self.transport_id.clone().ok_or_else(|| "Chromecast media channel is unavailable".to_string())?;
        let request_id = self.next_request_id();
        payload["requestId"] = json!(request_id);
        self.send_json(&transport_id, MEDIA_NAMESPACE, payload).await?;
        let response = self.wait_for_response(request_id, MEDIA_NAMESPACE).await?;
        let status = receiver_status_from_media(&response, &self.last_status)?;
        if response
            .get("status")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            self.media_session_id = None;
        } else if let Some(media_session_id) = response
            .pointer("/status/0/mediaSessionId")
            .and_then(Value::as_u64)
        {
            self.media_session_id = Some(media_session_id);
        }
        self.last_status = status.clone();
        Ok(status)
    }

    fn apply_receiver_volume(&mut self, receiver: &Value) {
        if let Some(volume) = receiver.pointer("/status/volume") {
            if let Some(level) = volume.get("level").and_then(Value::as_f64) { self.last_status.volume = Some((level * 100.0).clamp(0.0, 100.0)); }
            if let Some(muted) = volume.get("muted").and_then(Value::as_bool) { self.last_status.muted = Some(muted); }
        }
    }

    fn next_request_id(&mut self) -> u64 { self.request_id = self.request_id.saturating_add(1); self.request_id }

    async fn send_json(&mut self, destination_id: &str, namespace: &str, payload: Value) -> Result<(), String> {
        let payload = serde_json::to_string(&payload).map_err(|_| "could not encode Chromecast request".to_string())?;
        let message = CastMessage { source_id: SENDER_ID.to_string(), destination_id: destination_id.to_string(), namespace: namespace.to_string(), payload: Some(payload) };
        write_cast_message(&mut self.stream, &message).await
    }

    async fn wait_for_response(&mut self, request_id: u64, namespace: &str) -> Result<Value, String> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = time::timeout(remaining, read_cast_message(&mut self.stream))
                .await
                .map_err(|_| "Chromecast request timed out".to_string())??;
            if message.namespace == HEARTBEAT_NAMESPACE && message.payload_type().as_deref() == Some("PING") {
                self.send_json(&message.source_id, HEARTBEAT_NAMESPACE, json!({"type": "PONG"})).await?;
                continue;
            }
            if message.namespace == CONNECTION_NAMESPACE && message.payload_type().as_deref() == Some("CLOSE") {
                return Err("Chromecast receiver closed the connection".to_string());
            }
            if message.namespace != namespace { continue; }
            let payload = match message.payload.as_deref() { Some(payload) => parse_cast_json(payload)?, None => continue };
            if !response_matches_request(&payload, request_id, namespace) {
                // Cast V2 status notifications may be emitted independently of a command. Do not
                // let a delayed notification complete a newer in-flight request.
                continue;
            }
            if let Some(error_type) = payload.get("type").and_then(Value::as_str).filter(|kind| kind.contains("ERROR") || *kind == "LOAD_FAILED") {
                log::debug!("Chromecast receiver returned {error_type}");
                return Err("Chromecast rejected the request".to_string());
            }
            if payload.get("requestId").and_then(Value::as_u64) == Some(request_id) {
                return Ok(payload);
            }
            // Some third-party receivers (including older AirScreen builds) omit requestId from
            // status replies. There is only one in-flight request per adapter connection, so the
            // matching namespace and response kind are still unambiguous here.
            if namespace == RECEIVER_NAMESPACE && payload.get("type").and_then(Value::as_str) == Some("RECEIVER_STATUS") {
                log::debug!("Chromecast receiver status did not include requestId; accepting sequential response");
                return Ok(payload);
            }
            if namespace == MEDIA_NAMESPACE && payload.get("type").and_then(Value::as_str) == Some("MEDIA_STATUS") {
                log::debug!("Chromecast media status did not include requestId; accepting sequential response");
                return Ok(payload);
            }
        }
    }
}

#[derive(Debug)]
struct ChromecastCertificateVerifier;

impl ServerCertVerifier for ChromecastCertificateVerifier {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, RustlsError> { Ok(ServerCertVerified::assertion()) }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, signature: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, signature: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, RustlsError> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct CastMessage { source_id: String, destination_id: String, namespace: String, payload: Option<String> }

impl CastMessage {
    fn payload_type(&self) -> Option<String> { self.payload.as_deref().and_then(|payload| serde_json::from_str::<Value>(payload).ok()).and_then(|payload| payload.get("type").and_then(Value::as_str).map(str::to_string)) }
}

async fn write_cast_message(stream: &mut TlsStream<TcpStream>, message: &CastMessage) -> Result<(), String> {
    let frame = encode_cast_message(message)?;
    stream.write_all(&(frame.len() as u32).to_be_bytes()).await.map_err(|_| "Chromecast connection closed".to_string())?;
    stream.write_all(&frame).await.map_err(|_| "Chromecast connection closed".to_string())?;
    stream.flush().await.map_err(|_| "Chromecast connection closed".to_string())
}

async fn read_cast_message(stream: &mut TlsStream<TcpStream>) -> Result<CastMessage, String> {
    let size = stream.read_u32().await.map_err(|_| "Chromecast connection closed".to_string())? as usize;
    if size == 0 || size > MAX_CAST_FRAME { return Err("invalid Chromecast frame size".to_string()); }
    let mut frame = vec![0u8; size];
    stream.read_exact(&mut frame).await.map_err(|_| "truncated Chromecast frame".to_string())?;
    decode_cast_message(&frame)
}

fn encode_cast_message(message: &CastMessage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    put_varint_field(1, CAST_V2_PROTOCOL_VERSION, &mut out);
    put_string_field(2, &message.source_id, &mut out)?;
    put_string_field(3, &message.destination_id, &mut out)?;
    put_string_field(4, &message.namespace, &mut out)?;
    put_varint_field(5, CAST_STRING_PAYLOAD_TYPE, &mut out);
    if let Some(payload) = &message.payload { put_string_field(6, payload, &mut out)?; }
    Ok(out)
}

fn decode_cast_message(input: &[u8]) -> Result<CastMessage, String> {
    let mut offset = 0;
    let mut protocol_version = None;
    let mut source_id = None;
    let mut destination_id = None;
    let mut namespace = None;
    let mut payload_type = None;
    let mut payload = None;
    while offset < input.len() {
        let tag = read_varint(input, &mut offset)?;
        let field = tag >> 3;
        let wire = tag & 7;
        if wire == 2 {
            let len = read_varint(input, &mut offset)? as usize;
            let end = offset.checked_add(len).filter(|end| *end <= input.len()).ok_or_else(|| "invalid Chromecast protobuf length".to_string())?;
            if matches!(field, 2 | 3 | 4 | 6) {
                let value = std::str::from_utf8(&input[offset..end]).map_err(|_| "invalid Chromecast protobuf string".to_string())?.to_string();
                match field { 2 => source_id = Some(value), 3 => destination_id = Some(value), 4 => namespace = Some(value), 6 => payload = Some(value), _ => {} }
            }
            offset = end;
        } else if wire == 0 {
            let value = read_varint(input, &mut offset)?;
            match field { 1 => protocol_version = Some(value), 5 => payload_type = Some(value), _ => {} }
        }
        else { return Err("unsupported Chromecast protobuf field".to_string()); }
    }
    if protocol_version != Some(CAST_V2_PROTOCOL_VERSION) {
        return Err("unsupported Chromecast protocol version".to_string());
    }
    if payload_type != Some(CAST_STRING_PAYLOAD_TYPE) {
        return Err("unsupported Chromecast payload type".to_string());
    }
    Ok(CastMessage { source_id: source_id.ok_or_else(|| "Chromecast message has no source".to_string())?, destination_id: destination_id.unwrap_or_default(), namespace: namespace.ok_or_else(|| "Chromecast message has no namespace".to_string())?, payload })
}

fn put_varint_field(field: u64, value: u64, out: &mut Vec<u8>) { put_varint(field << 3, out); put_varint(value, out); }
fn put_string_field(field: u64, value: &str, out: &mut Vec<u8>) -> Result<(), String> { if value.len() > MAX_CAST_FRAME { return Err("Chromecast message field is too large".to_string()); } put_varint((field << 3) | 2, out); put_varint(value.len() as u64, out); out.extend_from_slice(value.as_bytes()); Ok(()) }
fn put_varint(mut value: u64, out: &mut Vec<u8>) { while value >= 0x80 { out.push((value as u8) | 0x80); value >>= 7; } out.push(value as u8); }
fn read_varint(input: &[u8], offset: &mut usize) -> Result<u64, String> { let mut result = 0u64; for shift in (0..64).step_by(7) { let byte = *input.get(*offset).ok_or_else(|| "truncated Chromecast protobuf".to_string())?; *offset += 1; result |= ((byte & 0x7f) as u64) << shift; if byte & 0x80 == 0 { return Ok(result); } } Err("invalid Chromecast protobuf varint".to_string()) }

fn parse_cast_json(payload: &str) -> Result<Value, String> { serde_json::from_str(payload).map_err(|_| "invalid Chromecast response".to_string()) }
fn response_matches_request(payload: &Value, request_id: u64, namespace: &str) -> bool {
    match payload.get("requestId").and_then(Value::as_u64) {
        Some(response_id) => response_id == request_id,
        None if namespace == RECEIVER_NAMESPACE => {
            payload.get("type").and_then(Value::as_str) == Some("RECEIVER_STATUS")
        }
        None if namespace == MEDIA_NAMESPACE => {
            payload.get("type").and_then(Value::as_str) == Some("MEDIA_STATUS")
        }
        None => false,
    }
}
fn media_command(kind: &str, media_session_id: Option<u64>, current_time: Option<f64>) -> Value { let mut request = json!({"type": kind}); if let Some(id) = media_session_id { request["mediaSessionId"] = json!(id); } if let Some(time) = current_time { request["currentTime"] = json!(time); } request }
fn default_receiver_transport_id(payload: &Value) -> Option<String> {
    payload
        .pointer("/status/applications")
        .and_then(Value::as_array)
        .and_then(|applications| applications.iter().find(|application| application.get("appId").and_then(Value::as_str) == Some(DEFAULT_MEDIA_RECEIVER_APP_ID)))
        .and_then(|application| application.get("transportId").and_then(Value::as_str))
        .map(str::to_string)
}
fn empty_status(phase: CastPhaseDto) -> CastReceiverStatus { CastReceiverStatus { phase, position: 0.0, duration: None, volume: None, muted: None, seekable: false } }
fn receiver_status_from_media(payload: &Value, fallback: &CastReceiverStatus) -> Result<CastReceiverStatus, String> {
    let statuses = payload.get("status").and_then(Value::as_array).ok_or_else(|| "Chromecast did not return media status".to_string())?;
    let Some(status) = statuses.first() else {
        // STOP and GET_STATUS with no active media session legitimately return an empty status
        // array. Preserve the last confirmed position so Core can restore local playback there.
        let mut stopped = fallback.clone();
        stopped.phase = CastPhaseDto::Stopped;
        stopped.seekable = false;
        return Ok(stopped);
    };
    let phase = match status.get("playerState").and_then(Value::as_str).unwrap_or("IDLE") { "PLAYING" => CastPhaseDto::Playing, "PAUSED" => CastPhaseDto::Paused, "BUFFERING" => CastPhaseDto::Buffering, _ => CastPhaseDto::Stopped };
    let position = status.get("currentTime").and_then(Value::as_f64).filter(|value| value.is_finite() && *value >= 0.0).unwrap_or(fallback.position);
    let duration = status.pointer("/media/duration").and_then(Value::as_f64).filter(|value| value.is_finite() && *value > 0.0).or(fallback.duration);
    let volume = status.pointer("/volume/level").and_then(Value::as_f64).map(|level| (level * 100.0).clamp(0.0, 100.0)).or(fallback.volume);
    let muted = status.pointer("/volume/muted").and_then(Value::as_bool).or(fallback.muted);
    Ok(CastReceiverStatus { phase, position, duration, volume, muted, seekable: duration.is_some() })
}
fn device_error(code: CastErrorCodeDto, message: &str, device_id: Option<&str>) -> CastErrorDto { CastErrorDto { code, message: message.to_string(), device_id: device_id.map(str::to_string) } }
fn cast_error(code: CastErrorCodeDto, detail: &str, device_id: &str) -> CastErrorDto { log::warn!("Chromecast adapter error for {device_id}: {detail}"); let message = match code { CastErrorCodeDto::DeviceDisconnected => "Chromecast disconnected", CastErrorCodeDto::LoadFailed => "Chromecast could not load this media", _ => "Chromecast command failed" }; device_error(code, message, Some(device_id)) }
fn unix_time_secs() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

#[derive(Default)]
struct MdnsService { instance: String, advertised: bool, port: Option<u16>, target: Option<String>, txt: Vec<String>, source: Option<IpAddr> }
struct DiscoveredChromecast { id: String, name: String, model_name: Option<String>, address: String, port: u16 }
#[derive(Debug)]
enum MdnsData { Ptr(String), Srv(u16, String), Txt(Vec<String>), Address(IpAddr), Other }
#[derive(Debug)]
struct MdnsRecord { name: String, data: MdnsData }

async fn discover_chromecasts() -> Result<Vec<DiscoveredChromecast>, String> {
    let (socket, multicast_listener) = create_mdns_socket().await?;
    let target: SocketAddr = MDNS_MULTICAST.parse().map_err(|_| "invalid mDNS multicast address".to_string())?;
    send_mdns_query(&socket, target, &[(CAST_SERVICE, 12)], !multicast_listener).await?;
    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    let mut services: HashMap<String, MdnsService> = HashMap::new();
    let mut addresses: HashMap<String, Vec<IpAddr>> = HashMap::new();
    let mut requested = HashSet::new();
    let mut buffer = [0u8; 9000];
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(Ok((size, source))) = time::timeout(remaining, socket.recv_from(&mut buffer)).await else { break; };
        let records = match parse_mdns_records(&buffer[..size]) { Ok(records) => records, Err(error) => { log::debug!("Malformed Chromecast mDNS response ignored: {error}"); continue; } };
        let mut questions = Vec::new();
        for record in records {
            match record.data {
                MdnsData::Ptr(instance) if record.name.eq_ignore_ascii_case(CAST_SERVICE) => {
                    let key = dns_key(&instance);
                    let service = services.entry(key.clone()).or_default();
                    service.instance = instance.clone(); service.advertised = true; service.source.get_or_insert(source.ip());
                    if requested.insert(format!("service:{key}")) { questions.extend([(instance.clone(), 33), (instance, 16)]); }
                }
                MdnsData::Srv(port, host) => { let key = dns_key(&record.name); let service = services.entry(key).or_default(); service.instance = record.name.clone(); service.port = Some(port); service.target = Some(host.clone()); if requested.insert(format!("host:{}", dns_key(&host))) { questions.extend([(host.clone(), 1), (host, 28)]); } }
                MdnsData::Txt(txt) => { let service = services.entry(dns_key(&record.name)).or_default(); service.instance = record.name; service.txt = txt; }
                MdnsData::Address(address) => { addresses.entry(dns_key(&record.name)).or_default().push(address); }
                _ => {}
            }
        }
        if !questions.is_empty() { let questions = questions.iter().map(|(name, kind)| (name.as_str(), *kind)).collect::<Vec<_>>(); send_mdns_query(&socket, target, &questions, !multicast_listener).await?; }
    }
    let mut devices = HashMap::new();
    for service in services.into_values().filter(|service| service.advertised) {
        let txt = txt_map(&service.txt);
        let Some(id) = txt.get("id").filter(|id| !id.is_empty()).cloned() else { continue; };
        let address = service.target.as_deref().and_then(|host| addresses.get(&dns_key(host))).and_then(|items| items.iter().find(|value| value.is_ipv4()).or_else(|| items.first())).copied().or(service.source).map(|address| address.to_string());
        let Some(address) = address else { continue; };
        let name = txt.get("fn").filter(|name| !name.is_empty()).cloned().unwrap_or_else(|| service_label(&service.instance));
        devices.entry(id.clone()).or_insert(DiscoveredChromecast { id, name, model_name: txt.get("md").cloned(), address, port: service.port.unwrap_or(CAST_PORT) });
    }
    let mut devices = devices.into_values().collect::<Vec<_>>();
    devices.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));
    Ok(devices)
}

async fn create_mdns_socket() -> Result<(UdpSocket, bool), String> {
    match create_multicast_mdns_socket() { Ok(socket) => Ok((socket, true)), Err(error) => { log::debug!("Chromecast mDNS multicast listener unavailable, requesting unicast replies: {error}"); Ok((UdpSocket::bind("0.0.0.0:0").await.map_err(|_| "could not bind mDNS socket".to_string())?, false)) } }
}
fn create_multicast_mdns_socket() -> Result<UdpSocket, String> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).map_err(|_| "could not create mDNS socket".to_string())?;
    socket.set_reuse_address(true).map_err(|_| "could not configure mDNS socket".to_string())?;
    #[cfg(unix)] socket.set_reuse_port(true).map_err(|_| "could not configure mDNS socket".to_string())?;
    socket.bind(&SocketAddr::from(([0, 0, 0, 0], 5353)).into()).map_err(|_| "could not bind mDNS multicast port".to_string())?;
    socket.join_multicast_v4(&MDNS_GROUP, &Ipv4Addr::UNSPECIFIED).map_err(|_| "could not join mDNS multicast group".to_string())?;
    socket.set_nonblocking(true).map_err(|_| "could not configure mDNS socket".to_string())?;
    let socket: std::net::UdpSocket = socket.into(); socket.set_multicast_loop_v4(true).map_err(|_| "could not configure mDNS socket".to_string())?; socket.set_multicast_ttl_v4(255).map_err(|_| "could not configure mDNS socket".to_string())?;
    UdpSocket::from_std(socket).map_err(|_| "could not initialize mDNS socket".to_string())
}
async fn send_mdns_query(socket: &UdpSocket, target: SocketAddr, questions: &[(&str, u16)], unicast: bool) -> Result<(), String> { let mut packet = Vec::new(); packet.extend_from_slice(&[0, 0, 0, 0]); packet.extend_from_slice(&(questions.len() as u16).to_be_bytes()); packet.extend_from_slice(&[0; 6]); for (name, kind) in questions { encode_dns_name(name, &mut packet)?; packet.extend_from_slice(&kind.to_be_bytes()); packet.extend_from_slice(&(if unicast { 0x8001u16 } else { 1u16 }).to_be_bytes()); } socket.send_to(&packet, target).await.map_err(|_| "could not send mDNS query".to_string()).map(|_| ()) }
fn parse_mdns_records(packet: &[u8]) -> Result<Vec<MdnsRecord>, String> { if packet.len() < 12 { return Ok(Vec::new()); } let mut offset = 12; let questions = read_u16(packet, 4)? as usize; let record_count = read_u16(packet, 6)? as usize + read_u16(packet, 8)? as usize + read_u16(packet, 10)? as usize; for _ in 0..questions { let (_, next) = read_dns_name(packet, offset)?; offset = next.checked_add(4).filter(|offset| *offset <= packet.len()).ok_or_else(|| "truncated mDNS question".to_string())?; } let mut records = Vec::new(); for _ in 0..record_count { let (name, next) = read_dns_name(packet, offset)?; offset = next; if offset + 10 > packet.len() { break; } let kind = read_u16(packet, offset)?; let len = read_u16(packet, offset + 8)? as usize; offset += 10; let end = offset.checked_add(len).filter(|end| *end <= packet.len()).ok_or_else(|| "truncated mDNS record".to_string())?; let data = match kind { 12 => MdnsData::Ptr(read_dns_name(packet, offset)?.0), 33 if len >= 6 => MdnsData::Srv(read_u16(packet, offset + 4)?, read_dns_name(packet, offset + 6)?.0), 16 => MdnsData::Txt(read_txt(&packet[offset..end])), 1 if len == 4 => MdnsData::Address(IpAddr::V4(Ipv4Addr::new(packet[offset], packet[offset + 1], packet[offset + 2], packet[offset + 3]))), 28 if len == 16 => { let mut bytes = [0; 16]; bytes.copy_from_slice(&packet[offset..end]); MdnsData::Address(IpAddr::V6(std::net::Ipv6Addr::from(bytes))) }, _ => MdnsData::Other }; records.push(MdnsRecord { name, data }); offset = end; } Ok(records) }
fn encode_dns_name(name: &str, out: &mut Vec<u8>) -> Result<(), String> { for label in name.trim_end_matches('.').split('.') { if label.len() > 63 { return Err("mDNS label is too long".to_string()); } out.push(label.len() as u8); out.extend_from_slice(label.as_bytes()); } out.push(0); Ok(()) }
fn read_dns_name(packet: &[u8], offset: usize) -> Result<(String, usize), String> { let mut labels = Vec::new(); let mut cursor = offset; let mut next = None; let mut jumps = 0; loop { let len = *packet.get(cursor).ok_or_else(|| "mDNS name exceeds packet".to_string())?; if len & 0xc0 == 0xc0 { let second = *packet.get(cursor + 1).ok_or_else(|| "truncated mDNS pointer".to_string())?; next.get_or_insert(cursor + 2); cursor = (((len & 0x3f) as usize) << 8) | second as usize; jumps += 1; if jumps > 16 { return Err("mDNS pointer loop".to_string()); } continue; } if len == 0 { return Ok((labels.join("."), next.unwrap_or(cursor + 1))); } cursor += 1; let end = cursor.checked_add(len as usize).filter(|end| *end <= packet.len()).ok_or_else(|| "truncated mDNS label".to_string())?; labels.push(String::from_utf8_lossy(&packet[cursor..end]).to_string()); cursor = end; } }
fn read_txt(data: &[u8]) -> Vec<String> { let mut values = Vec::new(); let mut offset = 0; while let Some(&len) = data.get(offset) { offset += 1; let end = offset + len as usize; if end > data.len() { break; } values.push(String::from_utf8_lossy(&data[offset..end]).to_string()); offset = end; } values }
fn read_u16(data: &[u8], offset: usize) -> Result<u16, String> { Ok(u16::from_be_bytes([*data.get(offset).ok_or_else(|| "truncated mDNS integer".to_string())?, *data.get(offset + 1).ok_or_else(|| "truncated mDNS integer".to_string())?])) }
fn dns_key(value: &str) -> String { value.trim_end_matches('.').to_ascii_lowercase() }
fn txt_map(items: &[String]) -> HashMap<String, String> { items.iter().filter_map(|item| item.split_once('=').map(|(key, value)| (key.to_ascii_lowercase(), value.to_string()))).collect() }
fn service_label(instance: &str) -> String { instance.split('.').next().filter(|value| !value.is_empty()).unwrap_or("Chromecast").to_string() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_v2_envelope_round_trips_without_protocol_leaks() {
        let message = CastMessage { source_id: SENDER_ID.to_string(), destination_id: RECEIVER_ID.to_string(), namespace: RECEIVER_NAMESPACE.to_string(), payload: Some(r#"{"type":"GET_STATUS","requestId":7}"#.to_string()) };
        assert_eq!(decode_cast_message(&encode_cast_message(&message).unwrap()).unwrap(), message);
    }

    #[test]
    fn cast_envelope_rejects_non_v2_protocol_versions() {
        let message = CastMessage { source_id: SENDER_ID.to_string(), destination_id: RECEIVER_ID.to_string(), namespace: RECEIVER_NAMESPACE.to_string(), payload: None };
        let mut frame = encode_cast_message(&message).unwrap();
        frame[1] = 1;

        assert_eq!(
            decode_cast_message(&frame).unwrap_err(),
            "unsupported Chromecast protocol version",
        );
    }

    #[test]
    fn media_status_maps_to_shared_receiver_status() {
        let status = receiver_status_from_media(&serde_json::from_str(r#"{"type":"MEDIA_STATUS","status":[{"mediaSessionId":3,"playerState":"PAUSED","currentTime":12.5,"media":{"duration":60},"volume":{"level":0.35,"muted":false}}]}"#).unwrap(), &empty_status(CastPhaseDto::Loading)).unwrap();
        assert_eq!(status.phase, CastPhaseDto::Paused); assert_eq!(status.position, 12.5); assert_eq!(status.duration, Some(60.0)); assert_eq!(status.volume, Some(35.0)); assert!(status.seekable);
    }

    #[test]
    fn empty_media_status_maps_to_stopped_and_preserves_position() {
        let fallback = CastReceiverStatus {
            phase: CastPhaseDto::Playing,
            position: 42.5,
            duration: Some(120.0),
            volume: Some(35.0),
            muted: Some(false),
            seekable: true,
        };
        let status = receiver_status_from_media(
            &serde_json::from_str(r#"{"type":"MEDIA_STATUS","status":[]}"#).unwrap(),
            &fallback,
        )
        .unwrap();

        assert_eq!(status.phase, CastPhaseDto::Stopped);
        assert_eq!(status.position, 42.5);
        assert_eq!(status.duration, Some(120.0));
        assert!(!status.seekable);
    }

    #[test]
    fn request_matching_rejects_delayed_status_notifications() {
        let delayed = json!({"type": "MEDIA_STATUS", "requestId": 6, "status": []});
        let compatible = json!({"type": "MEDIA_STATUS", "status": []});

        assert!(!response_matches_request(&delayed, 7, MEDIA_NAMESPACE));
        assert!(response_matches_request(&compatible, 7, MEDIA_NAMESPACE));
        assert!(!response_matches_request(&compatible, 7, RECEIVER_NAMESPACE));
    }

    #[test]
    fn tls_verifier_advertises_legacy_cast_rsa_signatures() {
        assert!(ChromecastCertificateVerifier
            .supported_verify_schemes()
            .contains(&SignatureScheme::RSA_PKCS1_SHA256));
    }

    #[test]
    fn txt_device_id_is_stable_and_deduplicable() {
        let values = txt_map(&["id=01234567-89ab-cdef-0123-456789abcdef".to_string(), "fn=Living Room".to_string(), "md=Chromecast".to_string()]);
        assert_eq!(values.get("id").unwrap(), "01234567-89ab-cdef-0123-456789abcdef"); assert_eq!(values.get("fn").unwrap(), "Living Room");
    }

    #[test]
    fn sanitized_mdns_fixture_contains_ptr_srv_txt_and_address() {
        let service = "_googlecast._tcp.local";
        let instance = "Living Room._googlecast._tcp.local";
        let host = "living-room.local";
        let mut packet = vec![0x84, 0x00, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0];
        let mut ptr = Vec::new(); encode_dns_name(instance, &mut ptr).unwrap(); append_record(&mut packet, service, 12, &ptr);
        let mut srv = vec![0, 0, 0, 0]; srv.extend_from_slice(&8009u16.to_be_bytes()); encode_dns_name(host, &mut srv).unwrap(); append_record(&mut packet, instance, 33, &srv);
        let mut txt = Vec::new(); for value in ["id=01234567-89ab-cdef-0123-456789abcdef", "fn=Living Room", "md=Chromecast"] { txt.push(value.len() as u8); txt.extend_from_slice(value.as_bytes()); } append_record(&mut packet, instance, 16, &txt);
        append_record(&mut packet, host, 1, &[192, 0, 2, 20]);
        let records = parse_mdns_records(&packet).unwrap();
        assert!(records.iter().any(|record| matches!(&record.data, MdnsData::Ptr(value) if value == instance)));
        assert!(records.iter().any(|record| matches!(&record.data, MdnsData::Srv(8009, value) if value == host)));
        assert!(records.iter().any(|record| matches!(&record.data, MdnsData::Address(IpAddr::V4(address)) if *address == Ipv4Addr::new(192, 0, 2, 20))));
    }

    fn append_record(packet: &mut Vec<u8>, name: &str, kind: u16, data: &[u8]) {
        encode_dns_name(name, packet).unwrap(); packet.extend_from_slice(&kind.to_be_bytes()); packet.extend_from_slice(&1u16.to_be_bytes()); packet.extend_from_slice(&120u32.to_be_bytes()); packet.extend_from_slice(&(data.len() as u16).to_be_bytes()); packet.extend_from_slice(data);
    }
}
