use crate::core::playlist_service::{PlaylistEntry, PlaylistSummary};
use crate::protocol::{
    CommandEnvelopeDto, CommandResultDto, GetPlaylistEntriesPageDto, PlayPlaylistEntryDto,
    PlaybackCommandDto, PlaylistEntriesPageDto, PlaylistEntryDto, PlaylistSummaryDto,
};
use crate::AppState;

#[tauri::command]
pub(crate) fn get_playlist_summaries(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PlaylistSummaryDto>, String> {
    state
        .playlist_service
        .list_summaries(&app)
        .map(|summaries| summaries.iter().map(to_summary_dto).collect())
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
        .list_summaries(&app)?
        .into_iter()
        .find(|summary| summary.id == request.playlist_id)
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
