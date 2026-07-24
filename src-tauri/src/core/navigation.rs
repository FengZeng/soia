use rand::Rng;
use serde::Deserialize;
use std::sync::Mutex;

/// Playlist entry synced from the frontend.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistEntry {
    pub path: String,
    pub title: Option<String>,
    pub added_at: f64,
}

/// A playlist synced from the frontend.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Playlist {
    pub id: String,
    pub entries: Vec<PlaylistEntry>,
}

/// Loop mode for playlist navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LoopMode {
    List,
    Shuffle,
}

/// Sort mode for playlist ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SortMode {
    Name,
    Added,
}

/// Navigation state synced from the frontend.
/// Core uses this read-only copy to resolve previous/next without round-tripping
/// through the Desktop Vue application.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NavigationState {
    pub playlists: Vec<Playlist>,
    pub active_playlist_id: Option<String>,
    pub playback_playlist_id: Option<String>,
    pub loop_mode: LoopMode,
    pub sort_mode: SortMode,
    pub is_loop_one: bool,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            playlists: Vec::new(),
            active_playlist_id: None,
            playback_playlist_id: None,
            loop_mode: LoopMode::List,
            sort_mode: SortMode::Name,
            is_loop_one: false,
        }
    }
}

/// Result of a navigation resolution attempt.
pub(crate) struct NavigationResult {
    pub path: String,
    pub title: Option<String>,
}

/// Holds the navigation state and provides resolution methods.
pub(crate) struct NavigationService {
    state: Mutex<NavigationState>,
}

impl NavigationService {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(NavigationState::default()),
        }
    }

    /// Replace the navigation state with a fresh sync from the frontend.
    /// Preserves the Core-owned `playback_playlist_id` — clients must not
    /// reset this field since Core determines it during navigation resolution.
    pub(crate) fn sync_state(&self, new_state: NavigationState) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let preserved_playback_id = state.playback_playlist_id.take();
        *state = new_state;
        // Restore Core-owned playback playlist if the sync didn't provide one
        if state.playback_playlist_id.is_none() {
            state.playback_playlist_id = preserved_playback_id;
        }
    }

    /// Update only the playback playlist ID (set when navigation resolves a playlist).
    #[allow(dead_code)]
    pub(crate) fn set_playback_playlist_id(&self, playlist_id: Option<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.playback_playlist_id = playlist_id;
    }

    /// Resolve the next or previous path from the playlist. Returns `None` if
    /// the current path is not found in any playlist.
    pub(crate) fn resolve_playlist_path(
        &self,
        current_path: &str,
        direction: i32,
    ) -> Option<NavigationResult> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        resolve_playlist_path_inner(&mut state, current_path, direction)
    }

    /// Resolve the path to play when the current track ends. Respects loop-one
    /// (returns `None` if loop-one is active, letting mpv handle the repeat).
    pub(crate) fn resolve_path_for_end(
        &self,
        current_path: &str,
    ) -> Option<NavigationResult> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        resolve_path_for_end_inner(&mut state, current_path)
    }

    /// Get the title for a path from the playlist that owns it.
    pub(crate) fn get_title_for_path(&self, path: &str) -> Option<String> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        get_title_for_path_inner(&state, path)
    }
}

// --- Internal resolution logic ---

fn resolve_playback_playlist_id<'a>(
    state: &'a NavigationState,
    current_path: &str,
) -> Option<&'a str> {
    let current = current_path.trim();
    if current.is_empty() {
        return None;
    }

    // Priority 1: current playback playlist
    if let Some(ref playback_id) = state.playback_playlist_id {
        if let Some(playlist) = state.playlists.iter().find(|p| p.id == *playback_id) {
            if playlist.entries.iter().any(|e| e.path == current) {
                return Some(&playlist.id);
            }
        }
    }

    // Priority 2: active (drawer-selected) playlist
    if let Some(ref active_id) = state.active_playlist_id {
        if let Some(playlist) = state.playlists.iter().find(|p| p.id == *active_id) {
            if playlist.entries.iter().any(|e| e.path == current) {
                return Some(&playlist.id);
            }
        }
    }

    // Priority 3: last playlist (reversed) that contains the path
    state
        .playlists
        .iter()
        .rev()
        .find(|p| p.entries.iter().any(|e| e.path == current))
        .map(|p| p.id.as_str())
}

fn get_ordered_entries(playlist: &Playlist, sort_mode: SortMode) -> Vec<&PlaylistEntry> {
    let mut entries: Vec<&PlaylistEntry> = playlist.entries.iter().collect();
    match sort_mode {
        SortMode::Name => {
            entries.sort_by(|a, b| {
                let a_name = a.title.as_deref().unwrap_or_else(|| path_display_name(&a.path));
                let b_name = b.title.as_deref().unwrap_or_else(|| path_display_name(&b.path));
                natural_sort_key(a_name).cmp(&natural_sort_key(b_name))
            });
        }
        SortMode::Added => {
            entries.sort_by(|a, b| {
                b.added_at
                    .partial_cmp(&a.added_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }
    entries
}

fn pick_random_index(length: usize, current_index: usize) -> usize {
    if length <= 1 {
        return 0;
    }
    let mut rng = rand::rng();
    loop {
        let next = rng.random_range(0..length);
        if next != current_index {
            return next;
        }
    }
}

fn resolve_playlist_path_inner(
    state: &mut NavigationState,
    current_path: &str,
    direction: i32,
) -> Option<NavigationResult> {
    let playlist_id = resolve_playback_playlist_id(state, current_path)?.to_string();
    // Update the playback playlist (mirrors TS behavior)
    state.playback_playlist_id = Some(playlist_id.clone());

    let playlist = state.playlists.iter().find(|p| p.id == playlist_id)?;
    let ordered = get_ordered_entries(playlist, state.sort_mode);
    if ordered.is_empty() {
        return None;
    }

    let current_index = ordered.iter().position(|e| e.path == current_path);

    if state.loop_mode == LoopMode::Shuffle {
        let ci = current_index.unwrap_or(0);
        let next_index = pick_random_index(ordered.len(), ci);
        let entry = ordered[next_index];
        return Some(NavigationResult {
            path: entry.path.clone(),
            title: entry.title.clone(),
        });
    }

    let ci = match current_index {
        Some(i) => i,
        None => {
            // Current path not found in sorted list: go to first or last
            let entry = if direction > 0 {
                ordered.first()?
            } else {
                ordered.last()?
            };
            return Some(NavigationResult {
                path: entry.path.clone(),
                title: entry.title.clone(),
            });
        }
    };

    let len = ordered.len();
    let next_index = if direction > 0 {
        (ci + 1) % len
    } else {
        (ci + len - 1) % len
    };
    let entry = ordered[next_index];
    Some(NavigationResult {
        path: entry.path.clone(),
        title: entry.title.clone(),
    })
}

fn resolve_path_for_end_inner(
    state: &mut NavigationState,
    current_path: &str,
) -> Option<NavigationResult> {
    if state.is_loop_one {
        return None;
    }

    let playlist_id = resolve_playback_playlist_id(state, current_path)?.to_string();
    state.playback_playlist_id = Some(playlist_id.clone());

    let playlist = state.playlists.iter().find(|p| p.id == playlist_id)?;
    let ordered = get_ordered_entries(playlist, state.sort_mode);
    if ordered.is_empty() {
        return None;
    }

    let current_index = ordered.iter().position(|e| e.path == current_path)?;

    if state.loop_mode == LoopMode::Shuffle {
        let next_index = pick_random_index(ordered.len(), current_index);
        let entry = ordered[next_index];
        return Some(NavigationResult {
            path: entry.path.clone(),
            title: entry.title.clone(),
        });
    }

    let next_index = (current_index + 1) % ordered.len();
    let entry = ordered[next_index];
    Some(NavigationResult {
        path: entry.path.clone(),
        title: entry.title.clone(),
    })
}

fn get_title_for_path_inner(state: &NavigationState, path: &str) -> Option<String> {
    let normalized = path.trim();
    if normalized.is_empty() {
        return None;
    }
    let playlist_id = resolve_playback_playlist_id(state, normalized)?;
    let playlist = state.playlists.iter().find(|p| p.id == playlist_id)?;
    let ordered = get_ordered_entries(playlist, state.sort_mode);
    let entry = ordered.iter().find(|e| e.path == normalized)?;
    entry.title.as_ref().and_then(|t| {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Extract the file name from a path (equivalent to TS `getPathDisplayName`).
fn path_display_name(path: &str) -> &str {
    if path.is_empty() {
        return path;
    }
    path.rsplit(&['/', '\\'][..])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

/// Produce a sort key that handles numeric substrings naturally.
/// Uses a simple approach: split into (text, number) segments for comparison.
fn natural_sort_key(input: &str) -> Vec<NaturalSegment> {
    let lower = input.to_lowercase();
    let mut segments = Vec::new();
    let mut chars = lower.chars().peekable();

    while chars.peek().is_some() {
        if chars.peek().map_or(false, |c| c.is_ascii_digit()) {
            let mut num_str = String::new();
            while chars.peek().map_or(false, |c| c.is_ascii_digit()) {
                num_str.push(chars.next().unwrap());
            }
            let num = num_str.parse::<u64>().unwrap_or(0);
            segments.push(NaturalSegment::Number(num));
        } else {
            let mut text = String::new();
            while chars.peek().map_or(false, |c| !c.is_ascii_digit()) {
                text.push(chars.next().unwrap());
            }
            segments.push(NaturalSegment::Text(text));
        }
    }
    segments
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NaturalSegment {
    Text(String),
    Number(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(path: &str, title: Option<&str>, added_at: f64) -> PlaylistEntry {
        PlaylistEntry {
            path: path.to_string(),
            title: title.map(|t| t.to_string()),
            added_at,
        }
    }

    fn make_playlist(id: &str, entries: Vec<PlaylistEntry>) -> Playlist {
        Playlist {
            id: id.to_string(),
            entries,
        }
    }

    fn make_state(playlists: Vec<Playlist>) -> NavigationState {
        NavigationState {
            playlists,
            active_playlist_id: None,
            playback_playlist_id: None,
            loop_mode: LoopMode::List,
            sort_mode: SortMode::Added,
            is_loop_one: false,
        }
    }

    #[test]
    fn resolves_next_in_list_mode() {
        let entries = vec![
            make_entry("/a.mp4", None, 3.0),
            make_entry("/b.mp4", None, 2.0),
            make_entry("/c.mp4", None, 1.0),
        ];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);

        let result = resolve_playlist_path_inner(&mut state, "/a.mp4", 1);
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/b.mp4"));
    }

    #[test]
    fn wraps_around_forward() {
        let entries = vec![
            make_entry("/a.mp4", None, 3.0),
            make_entry("/b.mp4", None, 2.0),
            make_entry("/c.mp4", None, 1.0),
        ];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);

        let result = resolve_playlist_path_inner(&mut state, "/c.mp4", 1);
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/a.mp4"));
    }

    #[test]
    fn wraps_around_backward() {
        let entries = vec![
            make_entry("/a.mp4", None, 3.0),
            make_entry("/b.mp4", None, 2.0),
            make_entry("/c.mp4", None, 1.0),
        ];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);

        let result = resolve_playlist_path_inner(&mut state, "/a.mp4", -1);
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/c.mp4"));
    }

    #[test]
    fn returns_none_when_path_not_in_any_playlist() {
        let entries = vec![make_entry("/a.mp4", None, 1.0)];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);

        let result = resolve_playlist_path_inner(&mut state, "/unknown.mp4", 1);
        assert!(result.is_none());
    }

    #[test]
    fn respects_loop_one_for_end() {
        let entries = vec![
            make_entry("/a.mp4", None, 2.0),
            make_entry("/b.mp4", None, 1.0),
        ];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);
        state.is_loop_one = true;

        let result = resolve_path_for_end_inner(&mut state, "/a.mp4");
        assert!(result.is_none());
    }

    #[test]
    fn end_advances_when_not_loop_one() {
        let entries = vec![
            make_entry("/a.mp4", None, 2.0),
            make_entry("/b.mp4", None, 1.0),
        ];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);

        let result = resolve_path_for_end_inner(&mut state, "/a.mp4");
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/b.mp4"));
    }

    #[test]
    fn prefers_playback_playlist() {
        let pl1 = make_playlist("pl1", vec![
            make_entry("/shared.mp4", Some("Title from PL1"), 1.0),
            make_entry("/next_pl1.mp4", None, 0.5),
        ]);
        let pl2 = make_playlist("pl2", vec![
            make_entry("/shared.mp4", Some("Title from PL2"), 2.0),
            make_entry("/next_pl2.mp4", None, 1.0),
        ]);
        let mut state = make_state(vec![pl1, pl2]);
        state.playback_playlist_id = Some("pl2".to_string());

        let result = resolve_playlist_path_inner(&mut state, "/shared.mp4", 1);
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/next_pl2.mp4"));
    }

    #[test]
    fn sorts_by_name_naturally() {
        let entries = vec![
            make_entry("/track10.mp4", None, 3.0),
            make_entry("/track2.mp4", None, 2.0),
            make_entry("/track1.mp4", None, 1.0),
        ];
        let playlist = make_playlist("pl1", entries);
        let mut state = make_state(vec![playlist]);
        state.sort_mode = SortMode::Name;

        // Natural order: track1, track2, track10
        // current = track1, next should be track2
        let result = resolve_playlist_path_inner(&mut state, "/track1.mp4", 1);
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/track2.mp4"));

        // current = track2, next should be track10
        let result = resolve_playlist_path_inner(&mut state, "/track2.mp4", 1);
        assert_eq!(result.as_ref().map(|r| r.path.as_str()), Some("/track10.mp4"));
    }

    #[test]
    fn get_title_returns_entry_title() {
        let entries = vec![
            make_entry("/a.mp4", Some("My Title"), 1.0),
            make_entry("/b.mp4", None, 0.5),
        ];
        let playlist = make_playlist("pl1", entries);
        let state = make_state(vec![playlist]);

        assert_eq!(
            get_title_for_path_inner(&state, "/a.mp4"),
            Some("My Title".to_string())
        );
        assert_eq!(get_title_for_path_inner(&state, "/b.mp4"), None);
    }

    #[test]
    fn path_display_name_extracts_filename() {
        assert_eq!(path_display_name("/foo/bar/baz.mp4"), "baz.mp4");
        assert_eq!(path_display_name("C:\\Users\\test\\file.mkv"), "file.mkv");
        assert_eq!(path_display_name("simple.mp4"), "simple.mp4");
        assert_eq!(path_display_name(""), "");
    }

    #[test]
    fn natural_sort_key_orders_numbers_correctly() {
        let mut items = vec!["track10", "track2", "track1", "track20"];
        items.sort_by(|a, b| natural_sort_key(a).cmp(&natural_sort_key(b)));
        assert_eq!(items, vec!["track1", "track2", "track10", "track20"]);
    }
}
