#![allow(dead_code)]

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

const CAST_LEASE_CAPACITY: usize = 128;
// CMAF uses one protected resource per init/segment file. At two-second segments this keeps
// roughly two hours of a long VOD available for HLS seeking without exhausting the registry.
const CAST_LEASE_RESOURCE_CAPACITY: usize = 4096;

/// A session-scoped LAN authorization. The token is opaque and never contains a path, URL, or
/// credentials. The media gateway will resolve `source_id` only after the client IP is accepted.
#[derive(Clone, Debug)]
pub(crate) struct CastMediaLease {
    pub(crate) token: String,
    pub(crate) session_id: String,
    pub(crate) source_id: String,
    source_ids: Vec<String>,
    resource_sources: HashMap<String, String>,
    receiver_ip: IpAddr,
    expires_at: Instant,
    last_access_at: Instant,
}

impl CastMediaLease {
    pub(crate) fn new(
        session_id: String,
        source_id: String,
        receiver_ip: IpAddr,
        lifetime: Duration,
    ) -> Self {
        Self::new_with_token(
            uuid::Uuid::new_v4().to_string(),
            session_id,
            source_id,
            receiver_ip,
            lifetime,
        )
    }

    pub(crate) fn new_with_token(
        token: String,
        session_id: String,
        source_id: String,
        receiver_ip: IpAddr,
        lifetime: Duration,
    ) -> Self {
        let now = Instant::now();
        Self {
            token,
            session_id,
            source_ids: vec![source_id.clone()],
            resource_sources: HashMap::new(),
            source_id,
            receiver_ip,
            expires_at: now + lifetime,
            last_access_at: now,
        }
    }

    pub(crate) fn media_path(&self) -> String {
        format!("/cast/{}/media", self.token)
    }

    pub(crate) fn resource_path(&self, source_id: &str) -> String {
        format!("/cast/{}/resource/{source_id}", self.token)
    }

    pub(crate) fn contains_source(&self, source_id: &str) -> bool {
        self.source_ids.iter().any(|id| id == source_id)
    }

    pub(crate) fn source_ids(&self) -> &[String] {
        &self.source_ids
    }

    fn add_source(&mut self, source_id: String) {
        if !self.contains_source(&source_id) {
            self.source_ids.push(source_id);
        }
    }

    fn resource_source(&self, origin: &str) -> Option<&str> {
        self.resource_sources.get(origin).map(String::as_str)
    }

    fn register_resource(&mut self, origin: String, source_id: String) -> ResourceRegistration {
        if let Some(existing) = self.resource_source(&origin) {
            return ResourceRegistration::Existing(existing.to_string());
        }
        if self.resource_sources.len() >= CAST_LEASE_RESOURCE_CAPACITY {
            return ResourceRegistration::AtCapacity;
        }
        self.add_source(source_id.clone());
        self.resource_sources.insert(origin, source_id);
        ResourceRegistration::Registered
    }

    fn is_authorized(&mut self, client_ip: IpAddr, now: Instant) -> bool {
        if client_ip != self.receiver_ip || now >= self.expires_at {
            return false;
        }
        self.last_access_at = now;
        true
    }

    fn is_expired(&self, now: Instant) -> bool {
        now >= self.expires_at
    }
}

/// Bounded registry for cast leases. All cleanup paths operate on an entire session so a reload,
/// device switch, disconnect, or app shutdown cannot leave a still-valid LAN URL behind.
pub(crate) struct CastMediaLeaseRegistry {
    leases: HashMap<String, CastMediaLease>,
}

pub(crate) enum ResourceRegistration {
    Registered,
    Existing(String),
    LeaseUnavailable,
    AtCapacity,
}

impl CastMediaLeaseRegistry {
    pub(crate) fn new() -> Self {
        Self {
            leases: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, lease: CastMediaLease) -> Result<(), String> {
        if self.leases.len() >= CAST_LEASE_CAPACITY {
            return Err("too many active cast media leases".to_string());
        }
        self.leases.insert(lease.token.clone(), lease);
        Ok(())
    }

    pub(crate) fn authorize(&mut self, token: &str, client_ip: IpAddr) -> Option<&CastMediaLease> {
        let now = Instant::now();
        let expired = self
            .leases
            .get(token)
            .is_some_and(|lease| lease.is_expired(now));
        if expired {
            self.leases.remove(token);
            return None;
        }
        let authorized = self
            .leases
            .get_mut(token)
            .is_some_and(|lease| lease.is_authorized(client_ip, now));
        if !authorized {
            return None;
        }
        self.leases.get(token)
    }

    pub(crate) fn revoke_session(&mut self, session_id: &str) -> Vec<CastMediaLease> {
        let revoked_tokens = self
            .leases
            .iter()
            .filter(|(_, lease)| lease.session_id == session_id)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        revoked_tokens
            .into_iter()
            .filter_map(|token| self.leases.remove(&token))
            .collect()
    }

    pub(crate) fn resource_source(&self, token: &str, origin: &str) -> Option<String> {
        self.leases
            .get(token)
            .filter(|lease| !lease.is_expired(Instant::now()))
            .and_then(|lease| lease.resource_source(origin))
            .map(str::to_string)
    }

    pub(crate) fn register_resource(
        &mut self,
        token: &str,
        origin: String,
        source_id: String,
    ) -> ResourceRegistration {
        let now = Instant::now();
        let Some(lease) = self.leases.get_mut(token) else {
            return ResourceRegistration::LeaseUnavailable;
        };
        if lease.is_expired(now) {
            return ResourceRegistration::LeaseUnavailable;
        }
        lease.register_resource(origin, source_id)
    }

    pub(crate) fn purge_expired(&mut self, now: Instant) -> Vec<CastMediaLease> {
        let expired_tokens = self
            .leases
            .iter()
            .filter(|(_, lease)| lease.is_expired(now))
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        expired_tokens
            .into_iter()
            .filter_map(|token| self.leases.remove(&token))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{CastMediaLease, CastMediaLeaseRegistry, ResourceRegistration, CAST_LEASE_RESOURCE_CAPACITY};
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    fn receiver_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20))
    }

    #[test]
    fn lease_url_is_opaque_and_bound_to_the_receiver_ip() {
        let mut leases = CastMediaLeaseRegistry::new();
        let lease = CastMediaLease::new_with_token(
            "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            "cast-session-1".to_string(),
            "source-token-1".to_string(),
            receiver_ip(),
            Duration::from_secs(60),
        );
        let media_path = lease.media_path();
        leases.insert(lease).unwrap();

        assert_eq!(media_path, "/cast/01234567-89ab-cdef-0123-456789abcdef/media");
        assert!(!media_path.contains("source-token-1"));
        assert!(leases.authorize("01234567-89ab-cdef-0123-456789abcdef", receiver_ip()).is_some());
        assert!(leases
            .authorize(
                "01234567-89ab-cdef-0123-456789abcdef",
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 21)),
            )
            .is_none());
        assert!(leases.authorize("01234567-89ab-cdef-0123-456789abcdef", receiver_ip()).is_some());
    }

    #[test]
    fn revoking_a_session_invalidates_every_lease_for_that_session() {
        let mut leases = CastMediaLeaseRegistry::new();
        for token in ["token-a", "token-b"] {
            leases
                .insert(CastMediaLease::new_with_token(
                    token.to_string(),
                    "cast-session-1".to_string(),
                    "source-token-1".to_string(),
                    receiver_ip(),
                    Duration::from_secs(60),
                ))
                .unwrap();
        }
        leases.revoke_session("cast-session-1");

        assert!(leases.authorize("token-a", receiver_ip()).is_none());
        assert!(leases.authorize("token-b", receiver_ip()).is_none());
    }

    #[test]
    fn lease_reuses_hls_resources_and_bounds_their_count() {
        let mut leases = CastMediaLeaseRegistry::new();
        leases
            .insert(CastMediaLease::new_with_token(
                "lease-token".to_string(),
                "cast-session-1".to_string(),
                "primary-source".to_string(),
                receiver_ip(),
                Duration::from_secs(60),
            ))
            .unwrap();
        assert!(matches!(
            leases.register_resource(
                "lease-token",
                "https://example.test/segment-0.ts".to_string(),
                "source-0".to_string(),
            ),
            ResourceRegistration::Registered,
        ));
        assert!(matches!(
            leases.register_resource(
                "lease-token",
                "https://example.test/segment-0.ts".to_string(),
                "duplicate-source".to_string(),
            ),
            ResourceRegistration::Existing(ref source) if source == "source-0",
        ));
        for index in 1..CAST_LEASE_RESOURCE_CAPACITY {
            assert!(matches!(
                leases.register_resource(
                    "lease-token",
                    format!("https://example.test/segment-{index}.ts"),
                    format!("source-{index}"),
                ),
                ResourceRegistration::Registered,
            ));
        }
        assert!(matches!(
            leases.register_resource(
                "lease-token",
                "https://example.test/segment-overflow.ts".to_string(),
                "source-overflow".to_string(),
            ),
            ResourceRegistration::AtCapacity,
        ));
    }
}
