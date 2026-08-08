use crate::core::navigation::{LoopMode, SortMode};
use crate::core::playlist_service::{PlaylistEntry, PlaylistSummary, PreparedPlaylistEntry};
use crate::protocol::{
    CommandEnvelopeDto, CommandResultDto, CreatePlaylistDto, GetPlaylistEntriesPageDto,
    PlayPlaylistEntryDto, PlaybackCommandDto, PlaylistEntriesPageDto, PlaylistEntryDto,
    PlaylistLoopModeDto, PlaylistMutationDto, PlaylistMutationResultDto, PlaylistSnapshotDto,
    PlaylistSortModeDto, PlaylistSummaryDto,
};
use crate::AppState;
use tauri::Emitter;

#[tauri::command]
pub(crate) fn get_playlist_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<PlaylistSnapshotDto, String> {
    state.playlist_service.snapshot(&app)
}

#[tauri::command]
pub(crate) fn create_playlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: CreatePlaylistDto,
) -> Result<PlaylistMutationResultDto, String> {
    let playlist = state.playlist_service.create_playlist_checked(
        &app,
        &request.name,
        revision_to_i64(request.expected_collection_revision)?,
    )?;
    publish_playlist_snapshot(
        &app,
        &state,
        Some(crate::protocol::PlaylistDto {
            summary: to_summary_dto(&playlist),
            entries: Vec::new(),
        }),
    )
}

#[tauri::command]
pub(crate) fn mutate_playlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    mutation: PlaylistMutationDto,
) -> Result<PlaylistMutationResultDto, String> {
    match mutation {
        PlaylistMutationDto::Rename { playlist_id, name, expected_playlist_revision } => {
            state.playlist_service.rename_playlist(&app, &playlist_id, &name, revision_to_i64(expected_playlist_revision)?)?;
        }
        PlaylistMutationDto::Delete { playlist_id, expected_playlist_revision, expected_collection_revision } => {
            state.playlist_service.delete_playlist_checked(
                &app,
                &playlist_id,
                revision_to_i64(expected_playlist_revision)?,
                revision_to_i64(expected_collection_revision)?,
            )?;
        }
        PlaylistMutationDto::AddEntries { playlist_id, entries, expected_playlist_revision } => {
            let entries = entries.into_iter().map(|entry| PreparedPlaylistEntry {
                path: entry.playback_key,
                title: entry.title,
                artwork_ref: entry.artwork_ref,
                added_at: crate::store::media_db::now_millis(),
            }).collect::<Vec<_>>();
            state.playlist_service.add_entries(&app, &playlist_id, &entries, revision_to_i64(expected_playlist_revision)?)?;
        }
        PlaylistMutationDto::RemoveEntries { playlist_id, entry_ids, expected_playlist_revision } => {
            state.playlist_service.remove_entries(&app, &playlist_id, &entry_ids, revision_to_i64(expected_playlist_revision)?)?;
        }
        PlaylistMutationDto::Clear { playlist_id, expected_playlist_revision } => {
            state.playlist_service.clear_entries(&app, &playlist_id, revision_to_i64(expected_playlist_revision)?)?;
        }
        PlaylistMutationDto::MoveEntry { playlist_id, entry_id, to_index, expected_playlist_revision } => {
            let to_index = u32::try_from(to_index).map_err(|_| "entry target index must not be negative".to_string())?;
            state.playlist_service.move_entry(&app, &playlist_id, &entry_id, to_index, revision_to_i64(expected_playlist_revision)?)?;
        }
        PlaylistMutationDto::ReorderPlaylists { playlist_ids, expected_collection_revision } => {
            state.playlist_service.reorder_playlists(&app, &playlist_ids, revision_to_i64(expected_collection_revision)?)?;
        }
        PlaylistMutationDto::SetPlaybackPlaylist { playlist_id } => {
            state.playlist_service.set_playback_preferences(&app, Some(playlist_id.as_deref()), None, None, None)?;
            state.navigation_service.set_playback_playlist_id(playlist_id);
        }
        PlaylistMutationDto::SetLoopMode { loop_mode } => {
            state.playlist_service.set_playback_preferences(&app, None, Some(loop_mode.clone()), None, None)?;
            state.navigation_service.set_loop_mode(if matches!(loop_mode, PlaylistLoopModeDto::Shuffle) { LoopMode::Shuffle } else { LoopMode::List });
        }
        PlaylistMutationDto::SetSortMode { sort_mode } => {
            state.playlist_service.set_playback_preferences(&app, None, None, Some(sort_mode.clone()), None)?;
            state.navigation_service.set_sort_mode(if matches!(sort_mode, PlaylistSortModeDto::Added) { SortMode::Added } else { SortMode::Name });
        }
        PlaylistMutationDto::SetLoopOne { is_loop_one } => {
            state.playlist_service.set_playback_preferences(&app, None, None, None, Some(is_loop_one))?;
            state.navigation_service.set_loop_one(is_loop_one);
        }
    }
    publish_playlist_snapshot(&app, &state, None)
}

#[tauri::command]
pub(crate) fn get_playlist_entries_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: GetPlaylistEntriesPageDto,
) -> Result<PlaylistEntriesPageDto, String> {
    let page = state.playlist_service.list_entries(
        &app,
        &request.playlist_id,
        request.offset,
        request.limit,
    )?;
    let summary = state
        .playlist_service
        .get_summary(&app, &request.playlist_id)?
        .ok_or_else(|| "playlist not found".to_string())?;
    Ok(PlaylistEntriesPageDto {
        playlist_id: summary.id,
        playlist_revision: non_negative_revision(summary.revision)?,
        total: page.total,
        offset: page.offset,
        entries: page.entries.iter().map(to_entry_dto).collect(),
    })
}

#[tauri::command]
pub(crate) async fn play_playlist_entry(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: PlayPlaylistEntryDto,
) -> Result<CommandResultDto, crate::protocol::CoreErrorDto> {
    let entry = state
        .playlist_service
        .get_entry(&app, &request.playlist_id, &request.entry_id)
        .map_err(|message| crate::protocol::CoreErrorDto::ExecutionFailed { message })?
        .ok_or_else(|| crate::protocol::CoreErrorDto::PlaylistNotFound {
            message: "playlist entry not found".to_string(),
            playlist_id: request.playlist_id.clone(),
        })?;
    state
        .navigation_service
        .set_playback_playlist_id(Some(request.playlist_id.clone()));
    crate::commands::navigation::execute_navigation_envelope(
        &app,
        &state,
        CommandEnvelopeDto {
            command_id: request.command_id,
            client_id: request.client_id,
            playback_session_id: None,
            command: PlaybackCommandDto::PlaySource {
                key: entry.path,
                title: entry.title,
            },
        },
    )
    .await
}

fn to_summary_dto(summary: &PlaylistSummary) -> PlaylistSummaryDto {
    PlaylistSummaryDto {
        id: summary.id.clone(),
        name: summary.name.clone(),
        created_at: summary.created_at,
        order_index: summary.order_index,
        revision: summary.revision.max(0) as u64,
        entry_count: summary.entry_count,
        is_protected: summary.is_protected,
    }
}

fn to_entry_dto(entry: &PlaylistEntry) -> PlaylistEntryDto {
    PlaylistEntryDto {
        id: entry.id.clone(),
        playback_key: entry.path.clone(),
        title: entry.title.clone(),
        artwork_ref: entry.artwork_ref.clone(),
        added_at: entry.added_at,
        order_index: entry.order_index,
        revision: entry.revision.max(0) as u64,
    }
}

fn non_negative_revision(revision: i64) -> Result<u64, String> {
    u64::try_from(revision).map_err(|_| "invalid negative playlist revision".to_string())
}

fn revision_to_i64(revision: u64) -> Result<i64, String> {
    i64::try_from(revision).map_err(|_| "playlist revision exceeds SQLite range".to_string())
}

fn publish_playlist_snapshot(
    app: &tauri::AppHandle,
    state: &AppState,
    playlist: Option<crate::protocol::PlaylistDto>,
) -> Result<PlaylistMutationResultDto, String> {
    let snapshot = state.playlist_service.publish_snapshot(app)?;
    app.emit("playlist-snapshot", &snapshot)
        .map_err(|error| error.to_string())?;
    Ok(PlaylistMutationResultDto {
        playlist,
        collection_revision: snapshot.collection_revision,
    })
}
