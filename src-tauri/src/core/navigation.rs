use crate::core::playlist_service::{
    PlaylistLoopMode, PlaylistNavigationContext, PlaylistSortMode,
};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopMode {
    List,
    Shuffle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortMode {
    Name,
    Added,
}

/// Core-initialized navigation context. SQLite owns playlist membership and entries.
#[derive(Clone, Debug)]
pub(crate) struct NavigationState {
    pub active_playlist_id: Option<String>,
    pub playback_playlist_id: Option<String>,
    pub loop_mode: LoopMode,
    pub sort_mode: SortMode,
    pub is_loop_one: bool,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            active_playlist_id: None,
            playback_playlist_id: None,
            loop_mode: LoopMode::List,
            sort_mode: SortMode::Name,
            is_loop_one: false,
        }
    }
}

/// Holds only client/runtime navigation choices. It never retains playlist entries.
pub(crate) struct NavigationService {
    state: Mutex<NavigationState>,
}

impl NavigationService {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(NavigationState::default()),
        }
    }

    pub(crate) fn initialize(&self, mut new_state: NavigationState) {
        if new_state.playback_playlist_id.is_none() {
            new_state.playback_playlist_id = new_state.active_playlist_id.clone();
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = new_state;
    }

    pub(crate) fn set_playback_playlist_id(&self, playlist_id: Option<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.playback_playlist_id = playlist_id;
    }

    pub(crate) fn set_loop_mode(&self, loop_mode: LoopMode) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.loop_mode = loop_mode;
    }

    pub(crate) fn set_sort_mode(&self, sort_mode: SortMode) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sort_mode = sort_mode;
    }

    pub(crate) fn set_loop_one(&self, is_loop_one: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_loop_one = is_loop_one;
    }

    pub(crate) fn playlist_navigation_context(&self) -> PlaylistNavigationContext {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        PlaylistNavigationContext {
            active_playlist_id: state.active_playlist_id.clone(),
            playback_playlist_id: state.playback_playlist_id.clone(),
            loop_mode: match state.loop_mode {
                LoopMode::List => PlaylistLoopMode::List,
                LoopMode::Shuffle => PlaylistLoopMode::Shuffle,
            },
            sort_mode: match state.sort_mode {
                SortMode::Name => PlaylistSortMode::Name,
                SortMode::Added => PlaylistSortMode::Added,
            },
            is_loop_one: state.is_loop_one,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_uses_the_core_playback_playlist() {
        let service = NavigationService::new();
        service.set_playback_playlist_id(Some("pl_1".to_string()));
        service.initialize(NavigationState {
            active_playlist_id: Some("pl_2".to_string()),
            playback_playlist_id: None,
            loop_mode: LoopMode::Shuffle,
            sort_mode: SortMode::Added,
            is_loop_one: false,
        });

        let context = service.playlist_navigation_context();
        assert_eq!(context.active_playlist_id.as_deref(), Some("pl_2"));
        assert_eq!(context.playback_playlist_id.as_deref(), Some("pl_2"));
        assert_eq!(context.loop_mode, PlaylistLoopMode::Shuffle);
        assert_eq!(context.sort_mode, PlaylistSortMode::Added);
    }
}
