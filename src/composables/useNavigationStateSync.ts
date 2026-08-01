import { watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { PlaylistLoopMode, PlaylistSortMode } from "../types/playlist";

type NavigationStateSyncOptions = {
    isHydrated: Ref<boolean>;
    activePlaylistId: Ref<string | null>;
    loopMode: Ref<PlaylistLoopMode>;
    sortMode: Ref<PlaylistSortMode>;
    isLoopOne: Ref<boolean>;
};

type NavigationStatePayload = {
    activePlaylistId: string | null;
    playbackPlaylistId: string | null;
    loopMode: "list" | "shuffle";
    sortMode: "name" | "added";
    isLoopOne: boolean;
};

/**
 * Syncs only compact UI navigation preferences. Rust Core resolves playlist
 * entries from SQLite at command execution time.
 */
export const useNavigationStateSync = ({
    isHydrated,
    activePlaylistId,
    loopMode,
    sortMode,
    isLoopOne,
}: NavigationStateSyncOptions) => {
    const syncToCore = () => {
        const payload: NavigationStatePayload = {
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
        [isHydrated, activePlaylistId, loopMode, sortMode, isLoopOne],
        () => {
            if (!isHydrated.value) return;
            syncToCore();
        },
        { deep: true, immediate: true },
    );
};
