import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CoreClientError } from "./CoreClient";
import type { DesktopPlaylistEditor } from "./PlaylistClient";
import type { CreatePlaylistDto } from "./generated/CreatePlaylistDto";
import type { GetPlaylistEntriesPageDto } from "./generated/GetPlaylistEntriesPageDto";
import type { PlaylistEntriesPageDto } from "./generated/PlaylistEntriesPageDto";
import type { PlaylistSnapshotDto } from "./generated/PlaylistSnapshotDto";
import type { PlayPlaylistEntryDto } from "./generated/PlayPlaylistEntryDto";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { PlaylistMutationDto } from "./generated/PlaylistMutationDto";
import type { PlaylistMutationResultDto } from "./generated/PlaylistMutationResultDto";
import { toCoreClientTransportError } from "./coreClientError";

const PLAYLIST_SNAPSHOT_EVENT = "playlist-snapshot";

export type PlaylistSnapshotListener = (snapshot: PlaylistSnapshotDto) => void;
export type PlaylistSnapshotErrorListener = (error: CoreClientError) => void;

/** Desktop transport adapter for the Core-owned playlist read/play boundary. */
export class TauriPlaylistClient implements DesktopPlaylistEditor {
    async getSnapshot(): Promise<PlaylistSnapshotDto> {
        try {
            return await invoke<PlaylistSnapshotDto>("get_playlist_snapshot");
        } catch (error) {
            throw toCoreClientTransportError(error, "failed to retrieve playlist snapshot");
        }
    }

    subscribe(
        listener: PlaylistSnapshotListener,
        onError?: PlaylistSnapshotErrorListener,
    ): () => void {
        let disposed = false;
        let unlisten: UnlistenFn | null = null;

        void listen<PlaylistSnapshotDto>(PLAYLIST_SNAPSHOT_EVENT, (event) => {
            if (!disposed) listener(event.payload);
        })
            .then((nextUnlisten) => {
                if (disposed) {
                    nextUnlisten();
                    return;
                }
                unlisten = nextUnlisten;
            })
            .catch((error) => {
                if (!disposed) {
                    onError?.(
                        toCoreClientTransportError(
                            error,
                            "failed to subscribe to playlist snapshots",
                        ),
                    );
                }
            });

        return () => {
            if (disposed) return;
            disposed = true;
            unlisten?.();
        };
    }

    async create(request: CreatePlaylistDto): Promise<PlaylistMutationResultDto> {
        try {
            return await invoke<PlaylistMutationResultDto>("create_playlist", { request });
        } catch (error) {
            throw toCoreClientTransportError(error, "failed to create playlist");
        }
    }

    async mutate(mutation: PlaylistMutationDto): Promise<PlaylistMutationResultDto> {
        try {
            return await invoke<PlaylistMutationResultDto>("mutate_playlist", { mutation });
        } catch (error) {
            throw toCoreClientTransportError(error, "failed to mutate playlist");
        }
    }

    async getEntriesPage(request: GetPlaylistEntriesPageDto): Promise<PlaylistEntriesPageDto> {
        try {
            return await invoke<PlaylistEntriesPageDto>("get_playlist_entries_page", { request });
        } catch (error) {
            throw toCoreClientTransportError(error, "failed to retrieve playlist entries");
        }
    }

    async playEntry(request: PlayPlaylistEntryDto): Promise<CommandResultDto> {
        try {
            return await invoke<CommandResultDto>("play_playlist_entry", { request });
        } catch (error) {
            throw toCoreClientTransportError(error, "failed to play playlist entry");
        }
    }
}

export const tauriPlaylistClient = new TauriPlaylistClient();
