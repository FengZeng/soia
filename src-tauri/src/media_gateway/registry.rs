use super::MediaSourceBackend;
use log::debug;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const MEDIA_SOURCE_IDLE_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const MEDIA_SOURCE_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const MEDIA_SOURCE_MAX_ENTRIES: usize = 8192;
const MEDIA_SOURCE_TARGET_ENTRIES: usize = 6144;

pub(super) struct MediaSourceEntry {
    pub(super) backend: Arc<dyn MediaSourceBackend>,
    pub(super) last_access: Instant,
}

pub(super) struct MediaSourceRegistry {
    pub(super) entries: HashMap<String, MediaSourceEntry>,
    last_cleanup: Instant,
}

impl MediaSourceRegistry {
    pub(super) fn new() -> Self {
        let now = Instant::now();
        Self {
            entries: HashMap::new(),
            last_cleanup: now,
        }
    }

    pub(super) fn insert(&mut self, token: String, backend: Arc<dyn MediaSourceBackend>) {
        let now = Instant::now();
        self.cleanup_if_due(now);
        self.entries.insert(
            token,
            MediaSourceEntry {
                backend,
                last_access: now,
            },
        );
        self.enforce_limit(now);
    }

    pub(super) fn get(&mut self, token: &str) -> Option<Arc<dyn MediaSourceBackend>> {
        let now = Instant::now();
        self.cleanup_if_due(now);
        let entry = self.entries.get_mut(token)?;
        entry.last_access = now;
        Some(entry.backend.clone())
    }

    #[allow(dead_code)] // Used by cast-session revocation once CastingService is registered.
    pub(super) fn remove(&mut self, token: &str) -> Option<Arc<dyn MediaSourceBackend>> {
        self.entries.remove(token).map(|entry| entry.backend)
    }

    pub(super) fn has_origin(&self, origin: &str) -> bool {
        self.entries
            .values()
            .any(|entry| entry.backend.origin() == origin)
    }

    pub(super) fn find_token_by_origin(&mut self, origin: &str) -> Option<String> {
        let now = Instant::now();
        self.cleanup_if_due(now);
        for (token, entry) in self.entries.iter_mut() {
            if entry.backend.origin() == origin {
                entry.last_access = now;
                return Some(token.clone());
            }
        }
        None
    }

    fn cleanup_if_due(&mut self, now: Instant) {
        if now.duration_since(self.last_cleanup) < MEDIA_SOURCE_CLEANUP_INTERVAL
            && self.entries.len() <= MEDIA_SOURCE_MAX_ENTRIES
        {
            return;
        }
        self.cleanup_idle(now);
    }

    pub(super) fn cleanup_idle(&mut self, now: Instant) {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| now.duration_since(entry.last_access) <= MEDIA_SOURCE_IDLE_TIMEOUT);
        self.last_cleanup = now;
        let removed = before.saturating_sub(self.entries.len());
        if removed > 0 {
            debug!("media gateway: cleaned up {removed} idle backend token(s)");
        }
    }

    fn enforce_limit(&mut self, now: Instant) {
        if self.entries.len() <= MEDIA_SOURCE_MAX_ENTRIES {
            return;
        }
        let remove_count = self
            .entries
            .len()
            .saturating_sub(MEDIA_SOURCE_TARGET_ENTRIES);
        let mut oldest = self
            .entries
            .iter()
            .map(|(token, entry)| (token.clone(), entry.last_access))
            .collect::<Vec<_>>();
        oldest.sort_by_key(|(_, last_access)| *last_access);
        for (token, _) in oldest.into_iter().take(remove_count) {
            self.entries.remove(&token);
        }
        self.last_cleanup = now;
        debug!("media gateway: evicted {remove_count} backend token(s) to enforce registry limit");
    }
}
