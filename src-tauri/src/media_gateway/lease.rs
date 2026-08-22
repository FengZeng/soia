#![allow(dead_code)]

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

const CAST_LEASE_CAPACITY: usize = 128;

/// A session-scoped LAN authorization. The token is opaque and never contains a path, URL, or
/// credentials. The media gateway will resolve `source_id` only after the client IP is accepted.
#[derive(Clone, Debug)]
pub(crate) struct CastMediaLease {
    pub(crate) token: String,
    pub(crate) session_id: String,
    pub(crate) source_id: String,
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

    fn new_with_token(
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
            source_id,
            receiver_ip,
            expires_at: now + lifetime,
            last_access_at: now,
        }
    }

    pub(crate) fn media_path(&self) -> String {
        format!("/cast/{}/media", self.token)
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

impl CastMediaLeaseRegistry {
    pub(crate) fn new() -> Self {
        Self {
            leases: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, lease: CastMediaLease) -> Result<(), String> {
        self.purge_expired(Instant::now());
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

    pub(crate) fn purge_expired(&mut self, now: Instant) {
        self.leases.retain(|_, lease| !lease.is_expired(now));
    }
}

#[cfg(test)]
mod tests {
    use super::{CastMediaLease, CastMediaLeaseRegistry};
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
}
