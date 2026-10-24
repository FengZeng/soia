use crate::protocol::{MediaTrackDto, PlaybackCommandDto};
use crate::store::track_preferences::{TrackKind, TrackPreferences};
use std::sync::Mutex;

#[derive(Default)]
struct RestoreState {
    generation: u64,
    playback_key: String,
    preferences: TrackPreferences,
    pending_audio: bool,
    pending_subtitle: bool,
    manual_audio: bool,
    manual_subtitle: bool,
    subtitles_disabled: bool,
    external_sources: Vec<(TrackKind, String, String)>,
}

#[derive(Default)]
pub(crate) struct TrackSelectionRuntime {
    state: Mutex<RestoreState>,
    external_requests: Mutex<Vec<super::external_tracks::ExternalTrackRequest>>,
}

pub(crate) fn selection_command(command: &PlaybackCommandDto) -> Option<(TrackKind, i64)> {
    match command {
        PlaybackCommandDto::SelectAudioTrack { track_id } => Some((TrackKind::Audio, *track_id)),
        PlaybackCommandDto::SelectSubtitleTrack { track_id } => Some((TrackKind::Subtitle, *track_id)),
        PlaybackCommandDto::DisableSubtitles => Some((TrackKind::Subtitle, 0)),
        _ => None,
    }
}

fn matching_track_id(kind: TrackKind, id: i64, path: Option<&str>, tracks: &[MediaTrackDto], sources: &[(TrackKind, String, String)]) -> Option<i64> {
    if kind == TrackKind::Subtitle && id == 0 { return Some(0); }
    tracks.iter().find(|track| {
        track.track_type == kind.mpv_type() && match path {
            Some(path) => track.external_filename.as_deref().is_some_and(|candidate| {
                same_path(candidate, path) || sources.iter().any(|(source_kind, source, url)| {
                    *source_kind == kind && same_path(source, path) && same_path(url, candidate)
                })
            }),
            None => track.id == id && track.external != Some(true),
        }
    }).map(|track| track.id)
}

pub(super) fn same_path(left: &str, right: &str) -> bool {
    let normalize = |path: &str| {
        url::Url::parse(path).ok().and_then(|url| url.to_file_path().ok())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()).replace('\\', "/")
    };
    normalize(left) == normalize(right)
}

impl TrackSelectionRuntime {
    pub(crate) fn is_current(&self, generation: u64, playback_key: &str) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.generation == generation && state.playback_key == playback_key
    }

    pub(crate) fn begin(&self, generation: u64, playback_key: String, preferences: TrackPreferences, subtitles_disabled: bool) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = RestoreState {
            generation,
            playback_key,
            pending_audio: preferences.selected_audio_track_id.is_some(),
            pending_subtitle: subtitles_disabled || preferences.selected_subtitle_track_id.is_some(),
            preferences,
            manual_subtitle: subtitles_disabled,
            manual_audio: false,
            subtitles_disabled,
            external_sources: Vec::new(),
        };
    }

    pub(crate) fn cancel_restore(&self, playback_key: &str, kind: TrackKind) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.playback_key != playback_key { return; }
        match kind {
            TrackKind::Audio => { state.pending_audio = false; state.manual_audio = true; }
            TrackKind::Subtitle => { state.pending_subtitle = false; state.manual_subtitle = true; }
        }
    }

    pub(crate) fn restore(
        &self,
        generation: u64,
        tracks: &[MediaTrackDto],
        mut set_track: impl FnMut(TrackKind, i64) -> bool,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation != generation { return; }
        for kind in [TrackKind::Audio, TrackKind::Subtitle] {
            let (pending, id, path) = match kind {
                TrackKind::Audio => (state.pending_audio, state.preferences.selected_audio_track_id, state.preferences.selected_audio_track_path.as_deref()),
                TrackKind::Subtitle if state.subtitles_disabled => (state.pending_subtitle, Some(0), None),
                TrackKind::Subtitle => (state.pending_subtitle, state.preferences.selected_subtitle_track_id, state.preferences.selected_subtitle_track_path.as_deref()),
            };
            let Some(id) = id.filter(|_| pending) else { continue };
            let Some(id) = matching_track_id(kind, id, path, tracks, &state.external_sources) else { continue };
            if set_track(kind, id) {
                match kind {
                    TrackKind::Audio => state.pending_audio = false,
                    TrackKind::Subtitle => state.pending_subtitle = false,
                }
            }
        }
    }

    pub(crate) fn saved_external_tracks(
        &self,
        generation: u64,
        tracks: &[MediaTrackDto],
    ) -> Vec<(TrackKind, String)> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation != generation { return Vec::new(); }
        let mut requests = Vec::new();
        for (kind, id, path, manual) in [
            (TrackKind::Audio, state.preferences.selected_audio_track_id, state.preferences.selected_audio_track_path.as_deref(), state.manual_audio),
            (TrackKind::Subtitle, state.preferences.selected_subtitle_track_id, state.preferences.selected_subtitle_track_path.as_deref(), state.manual_subtitle),
        ] {
            if let (Some(id), Some(path)) = (id, path) {
                if !manual && matching_track_id(kind, id, Some(path), tracks, &state.external_sources).is_none() {
                    requests.push((kind, path.to_string()));
                }
            }
        }
        requests
    }

    pub(crate) fn register_external_source(&self, generation: u64, kind: TrackKind, source: &str, url: &str) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation != generation { return false; }
        if !state.external_sources.iter().any(|entry| entry.0 == kind && entry.1 == source && entry.2 == url) {
            state.external_sources.push((kind, source.to_string(), url.to_string()));
        }
        true
    }

    pub(crate) fn external_source(&self, playback_key: &str, kind: TrackKind, url: &str) -> Result<String, String> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.playback_key != playback_key { return Err("playback source changed".into()); }
        if let Some((_, source, _)) = state.external_sources.iter().find(|entry| entry.0 == kind && same_path(&entry.2, url)) {
            return Ok(source.clone());
        }
        if super::external_tracks::is_temporary_source(url) {
            return Err("cannot persist an external track without its original source".into());
        }
        Ok(url.to_string())
    }

    pub(crate) fn external_track_id(&self, generation: u64, kind: TrackKind, source: &str, tracks: &[MediaTrackDto]) -> Option<i64> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation != generation { return None; }
        matching_track_id(kind, 1, Some(source), tracks, &state.external_sources)
    }

    pub(crate) fn enqueue_external_track(&self, request: super::external_tracks::ExternalTrackRequest) {
        self.external_requests.lock().unwrap_or_else(|p| p.into_inner()).push(request);
    }

    pub(crate) fn take_external_requests(&self) -> Vec<super::external_tracks::ExternalTrackRequest> {
        std::mem::take(&mut *self.external_requests.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub(crate) fn select_added_track(&self, generation: u64, kind: TrackKind, id: i64, select: bool, set_track: impl FnOnce()) {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation != generation || !select { return; }
        let preserve = match kind {
            TrackKind::Audio => state.manual_audio || state.preferences.selected_audio_track_id.is_some(),
            TrackKind::Subtitle => state.manual_subtitle || state.preferences.selected_subtitle_track_id.is_some(),
        };
        if !preserve && id > 0 { set_track(); }
    }

    pub(crate) fn carry(
        &self,
        generation: u64,
        sid: i64,
        aid: i64,
        mut set_track: impl FnMut(TrackKind, i64),
    ) {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation != generation { return; }
        if state.preferences.selected_subtitle_track_id.is_none() && !state.manual_subtitle {
            set_track(TrackKind::Subtitle, sid);
        }
        if aid > 0 && state.preferences.selected_audio_track_id.is_none() && !state.manual_audio {
            set_track(TrackKind::Audio, aid);
        }
    }
}

pub(crate) async fn load_external_track(
    app: &tauri::AppHandle,
    state: &crate::AppState,
    playback_key: &str,
    kind: TrackKind,
    path: &str,
    select: bool,
    title: &str,
) -> Result<bool, String> {
    let generation = state.playback_load_coordinator.current();
    super::external_tracks::load(app, state, generation, playback_key, kind, path, select, title).await
}

pub(crate) async fn run_legacy_command(
    app: &tauri::AppHandle,
    state: &crate::AppState,
    args: &[&str],
) -> Result<(), String> {
    let manual_add = match (args.first(), args.get(2)) {
        (Some(&"audio-add"), Some(&"select")) => Some(TrackKind::Audio),
        (Some(&"sub-add"), Some(&"select")) => Some(TrackKind::Subtitle),
        _ => None,
    };
    let _admission_lock = if manual_add.is_some() {
        Some(state.playback_command_lock.lock().await)
    } else {
        None
    };
    let snapshot = state.playback_state.current();
    if let (Some(kind), Some(key)) = (manual_add, snapshot.playback_key.as_deref()) {
        state.track_selection.cancel_restore(key, kind);
    }
    let mpv = state.mpv_player.lock().map_err(|e| e.to_string())?;
    crate::mpv_command_checked(&mpv, args)?;
    if let Some(kind) = manual_add {
        let track_id = mpv.get_property_string(kind.property())?
            .parse::<i64>().map_err(|e| e.to_string())?;
        let source = args.get(1).map(|source| state.track_selection.external_source(
            snapshot.playback_key.as_deref().unwrap_or(""), kind, source,
        )).transpose()?;
        crate::store::track_preferences::save(app, &snapshot, kind, track_id, source.as_deref())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks() -> Vec<MediaTrackDto> {
        vec![
            MediaTrackDto { id: 2, track_type: "audio".into(), ..Default::default() },
            MediaTrackDto { id: 3, track_type: "sub".into(), ..Default::default() },
        ]
    }

    fn preferences() -> TrackPreferences {
        TrackPreferences {
            selected_audio_track_id: Some(2), selected_subtitle_track_id: Some(3),
            ..Default::default()
        }
    }

    fn restore(runtime: &TrackSelectionRuntime, generation: u64, tracks: &[MediaTrackDto]) -> Vec<(TrackKind, i64)> {
        let mut applied = Vec::new();
        runtime.restore(generation, tracks, |kind, id| { applied.push((kind, id)); true });
        applied
    }

    #[test]
    fn restores_saved_tracks_once_and_blocks_episode_carry_over() {
        let runtime = TrackSelectionRuntime::default();
        runtime.begin(1, "video".into(), preferences(), false);
        assert_eq!(restore(&runtime, 1, &tracks()), vec![(TrackKind::Audio, 2), (TrackKind::Subtitle, 3)]);
        assert!(restore(&runtime, 1, &tracks()).is_empty());
        runtime.carry(1, 1, 1, |_, _| panic!("saved preferences must take priority"));
        runtime.select_added_track(1, TrackKind::Audio, 4, true, || panic!("must preserve saved audio"));
        runtime.select_added_track(1, TrackKind::Subtitle, 4, true, || panic!("must preserve saved subtitle"));
    }

    #[test]
    fn automatic_choices_and_episode_carry_remain_available_without_preferences() {
        let runtime = TrackSelectionRuntime::default();
        runtime.begin(1, "video".into(), TrackPreferences::default(), false);
        assert!(restore(&runtime, 1, &tracks()).is_empty());
        let mut carried = Vec::new();
        runtime.carry(1, 3, 0, |kind, id| carried.push((kind, id)));
        assert_eq!(carried, vec![(TrackKind::Subtitle, 3)]);
        let mut selected = false;
        runtime.select_added_track(1, TrackKind::Subtitle, 4, true, || selected = true);
        assert!(selected);
    }

    #[test]
    fn off_is_distinct_from_auto_and_global_disable_overrides_subtitle_restore() {
        let runtime = TrackSelectionRuntime::default();
        runtime.begin(1, "video".into(), TrackPreferences { selected_subtitle_track_id: Some(0), ..preferences() }, false);
        assert_eq!(restore(&runtime, 1, &[]), vec![(TrackKind::Subtitle, 0)]);
        runtime.begin(2, "video".into(), preferences(), true);
        assert_eq!(restore(&runtime, 2, &tracks()), vec![(TrackKind::Audio, 2), (TrackKind::Subtitle, 0)]);
    }

    #[test]
    fn external_tracks_match_by_path_when_ids_change() {
        let runtime = TrackSelectionRuntime::default();
        let prefs = TrackPreferences {
            selected_subtitle_track_id: Some(3),
            selected_subtitle_track_path: Some("/subs/a.ass".into()),
            ..Default::default()
        };
        runtime.begin(1, "video".into(), prefs, false);
        assert!(restore(&runtime, 1, &tracks()).is_empty());
        let external = runtime.saved_external_tracks(1, &tracks());
        assert_eq!(external, vec![(TrackKind::Subtitle, "/subs/a.ass".into())]);
        let external_track = MediaTrackDto {
            id: 7, track_type: "sub".into(), external: Some(true),
            external_filename: Some("file:///subs/a.ass".into()), ..Default::default()
        };
        assert_eq!(restore(&runtime, 1, &[external_track]), vec![(TrackKind::Subtitle, 7)]);
    }

    #[test]
    fn manual_selection_and_replaced_sessions_cancel_pending_restoration() {
        let runtime = TrackSelectionRuntime::default();
        runtime.begin(1, "video-a".into(), preferences(), false);
        runtime.cancel_restore("video-a", TrackKind::Subtitle);
        assert_eq!(restore(&runtime, 1, &tracks()), vec![(TrackKind::Audio, 2)]);
        runtime.begin(2, "video-b".into(), preferences(), false);
        assert!(restore(&runtime, 1, &tracks()).is_empty());
        runtime.cancel_restore("video-a", TrackKind::Audio);
        assert_eq!(restore(&runtime, 2, &tracks()), vec![(TrackKind::Audio, 2), (TrackKind::Subtitle, 3)]);
    }

    #[test]
    fn missing_tracks_fall_back_and_failed_restores_can_retry() {
        let runtime = TrackSelectionRuntime::default();
        runtime.begin(1, "video".into(), preferences(), false);
        assert!(restore(&runtime, 1, &[]).is_empty());
        runtime.restore(1, &tracks(), |_, _| false);
        assert_eq!(restore(&runtime, 1, &tracks()).len(), 2);
    }

    #[test]
    fn only_explicit_selection_commands_are_persisted() {
        assert_eq!(selection_command(&PlaybackCommandDto::SelectAudioTrack { track_id: 2 }), Some((TrackKind::Audio, 2)));
        assert_eq!(selection_command(&PlaybackCommandDto::SelectSubtitleTrack { track_id: 3 }), Some((TrackKind::Subtitle, 3)));
        assert_eq!(selection_command(&PlaybackCommandDto::DisableSubtitles), Some((TrackKind::Subtitle, 0)));
        assert_eq!(selection_command(&PlaybackCommandDto::SetPaused { paused: false }), None);
    }

    #[test]
    fn delayed_external_completion_respects_manual_choice_and_playback_generation() {
        let runtime = TrackSelectionRuntime::default();
        runtime.begin(1, "video-a".into(), TrackPreferences::default(), false);
        let requests = runtime.saved_external_tracks(1, &[]);
        assert!(requests.is_empty());
        runtime.cancel_restore("video-a", TrackKind::Subtitle);
        runtime.select_added_track(1, TrackKind::Subtitle, 9, true, || panic!("late load replaced manual choice"));
        runtime.begin(2, "video-b".into(), TrackPreferences::default(), false);
        assert!(!runtime.register_external_source(1, TrackKind::Subtitle, "old.ass", "old.ass"));
        runtime.select_added_track(1, TrackKind::Subtitle, 9, true, || panic!("late load selected a track on another video"));
        assert!(!runtime.is_current(1, "video-a"));
        assert!(runtime.is_current(2, "video-b"));
    }
}
