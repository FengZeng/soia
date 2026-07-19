import { watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Playlist, PlaylistLoopMode, PlaylistSortMode } from "../types/playlist";

type NavigationStateSyncOptions = {
    playlists: Ref<Playlist[]>;
    activePlaylistId: Ref<string | null>;
    loopMode: Ref<PlaylistLoopMode>;
    sortMode: Ref<PlaylistSortMode>;
    isLoopOne: Ref<boolean>;
};

type NavigationStatePayload = {
    playlists: { id: string; entries: { path: string; title?: string; addedAt: number }[] }[];
    activePlaylistId: string | null;
    playbackPlaylistId: string | null;
    loopMode: "list" | "shuffle";
    sortMode: "name" | "added";
    isLoopOne: boolean;
};

/**
 * Syncs playlist/navigation state from the frontend to Rust Core.
 * Core uses this state to resolve previous/next navigation without requiring
 * the Desktop Vue application as an intermediary.
 */
export const useNavigationStateSync = ({
    playlists,
    activePlaylistId,
    loopMode,
    sortMode,
    isLoopOne,
}: NavigationStateSyncOptions) => {
    const syncToCore = () => {
        const payload: NavigationStatePayload = {
            playlists: playlists.value.map((pl) => ({
                id: pl.id,
                entries: pl.entries.map((entry) => ({
                    path: entry.path,
                    title: entry.title,
                    addedAt: entry.addedAt,
                })),
            })),
            activePlaylistId: activePlaylistId.value,
            playbackPlaylistId: null,
            loopMode: loopMode.value,
            sortMode: sortMode.value,
            isLoopOne: isLoopOne.value,
        };
        invoke("sync_navigation_state", { payload }).catch(() => {
            // Silently ignore sync failures — navigation will fall back to
            // directory adjacency if Core has stale state.
        });
    };

    watch(
        [playlists, activePlaylistId, loopMode, sortMode, isLoopOne],
        syncToCore,
        { deep: true, immediate: true },
    );
};
