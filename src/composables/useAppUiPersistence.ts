import { onUnmounted, ref, watch, type Ref } from "vue";
import type { PlaylistScrollState } from "../types/playlist";
import { applyThemeFromSettingGroups } from "../constants/theme";
import { applyWindowDecorationsFromSettingGroups } from "../constants/windowDecorations";
import {
    createDebouncedUiStateSaver,
    loadUiState,
    saveUiState,
} from "./useUiStateStore";

type StoredSettingGroup = {
    title: string;
    items: Array<{ label: string; value: string }>;
};

const PLAYLIST_STATE_STORAGE_KEY = "soia.playlists.v2";

type PlaylistPersistApi = {
    loadFromCore: () => Promise<void>;
    applyPersistedState: (stored: { activePlaylistId?: string | null }) => void;
    toPersistedState: () => {
        activePlaylistId: string | null;
    };
};

type PlaylistPersistedPayload = {
    activePlaylistId: string | null;
    playlistScrollState: PlaylistScrollState;
    playlistDrawerWidthRatio: number | null;
};

const normalizePersistedActivePlaylistId = (value: string | null | undefined) => {
    if (typeof value !== "string") return null;
    const trimmed = value.trim();
    return trimmed ? trimmed : null;
};

const normalizeScrollTop = (value: unknown) => {
    if (typeof value !== "number" || !Number.isFinite(value)) return 0;
    if (value <= 0) return 0;
    return Math.round(value);
};

const normalizePlaylistScrollState = (value: unknown): PlaylistScrollState => {
    const fallback: PlaylistScrollState = {
        list: 0,
        playlists: {},
    };
    if (!value || typeof value !== "object") return fallback;
    const candidate = value as {
        list?: unknown;
        playlists?: unknown;
    };
    const playlists: Record<string, number> = {};
    if (candidate.playlists && typeof candidate.playlists === "object") {
        Object.entries(candidate.playlists as Record<string, unknown>).forEach(
            ([playlistId, scrollTop]) => {
                const normalizedTop = normalizeScrollTop(scrollTop);
                if (!playlistId.trim()) return;
                playlists[playlistId] = normalizedTop;
            },
        );
    }
    return {
        list: normalizeScrollTop(candidate.list),
        playlists,
    };
};

const normalizePlaylistDrawerWidthRatio = (value: unknown): number | null => {
    if (typeof value !== "number" || !Number.isFinite(value)) return null;
    if (value <= 0) return null;
    return Math.round(Math.min(value, 0.86) * 10000) / 10000;
};

const savePlaylistStateToLocal = (payload: PlaylistPersistedPayload) => {
    if (typeof window === "undefined") return;
    try {
        window.localStorage.setItem(
            PLAYLIST_STATE_STORAGE_KEY,
            JSON.stringify(payload),
        );
    } catch {
        // Ignore localStorage write failures.
    }
};

const loadPlaylistStateFromLocal = (): Partial<PlaylistPersistedPayload> | null => {
    if (typeof window === "undefined") return null;
    try {
        const raw = window.localStorage.getItem(PLAYLIST_STATE_STORAGE_KEY);
        if (!raw) return null;
        const parsed = JSON.parse(raw) as Partial<PlaylistPersistedPayload> | null;
        if (!parsed || typeof parsed !== "object") return null;
        return parsed;
    } catch {
        return null;
    }
};

type UseAppUiPersistenceOptions<PanelId extends string> = {
    activePanel: Ref<PanelId>;
    activePlaylistId: Ref<string | null>;
    playlistScrollState: Ref<PlaylistScrollState>;
    playlistDrawerWidthRatio: Ref<number | null>;
    playlistState: PlaylistPersistApi;
    schedulePointerRefresh: () => void;
    normalizeStoredPanel: (panel: string) => PanelId;
};

export const useAppUiPersistence = <PanelId extends string>({
    activePanel,
    activePlaylistId,
    playlistScrollState,
    playlistDrawerWidthRatio,
    playlistState,
    schedulePointerRefresh,
    normalizeStoredPanel,
}: UseAppUiPersistenceOptions<PanelId>) => {
    const hasLoadedPanel = ref(false);
    const scrollStateSaver = createDebouncedUiStateSaver(220);
    const hasOwn = <T extends object, K extends PropertyKey>(
        value: T | null | undefined,
        key: K,
    ) => !!value && Object.prototype.hasOwnProperty.call(value, key);

    const loadActivePanel = async () => {
        const stored = await loadUiState<{
            activePanel?: string;
            activePlaylistId?: string | null;
            playlistScrollState?: PlaylistScrollState;
            playlistDrawerWidthRatio?: number | null;
            settings?: {
                groups?: StoredSettingGroup[];
            };
        }>();
        if (stored?.activePanel) {
            activePanel.value = normalizeStoredPanel(stored.activePanel);
        }
        await playlistState.loadFromCore();
        const localPlaylistState = loadPlaylistStateFromLocal();
        const playlistStateFromStorage = {
            activePlaylistId: hasOwn(stored, "activePlaylistId")
                ? normalizePersistedActivePlaylistId(stored?.activePlaylistId)
                : normalizePersistedActivePlaylistId(
                      localPlaylistState?.activePlaylistId,
                  ),
            playlistScrollState: hasOwn(stored, "playlistScrollState")
                ? normalizePlaylistScrollState(stored?.playlistScrollState)
                : normalizePlaylistScrollState(
                      localPlaylistState?.playlistScrollState,
                  ),
            playlistDrawerWidthRatio: hasOwn(stored, "playlistDrawerWidthRatio")
                ? normalizePlaylistDrawerWidthRatio(
                      stored?.playlistDrawerWidthRatio,
                  )
                : normalizePlaylistDrawerWidthRatio(
                      localPlaylistState?.playlistDrawerWidthRatio,
                  ),
        };
        playlistState.applyPersistedState({
            ...playlistStateFromStorage,
        });
        playlistScrollState.value = playlistStateFromStorage.playlistScrollState;
        playlistDrawerWidthRatio.value =
            playlistStateFromStorage.playlistDrawerWidthRatio;
        applyThemeFromSettingGroups(stored?.settings?.groups);
        void applyWindowDecorationsFromSettingGroups(stored?.settings?.groups);
        hasLoadedPanel.value = true;
    };

    const savePlaylistState = () => {
        if (!hasLoadedPanel.value) return;
        const persisted = playlistState.toPersistedState();
        const normalizedScrollState = normalizePlaylistScrollState(
            playlistScrollState.value,
        );
        savePlaylistStateToLocal({
            activePlaylistId: persisted.activePlaylistId,
            playlistScrollState: normalizedScrollState,
            playlistDrawerWidthRatio: normalizePlaylistDrawerWidthRatio(
                playlistDrawerWidthRatio.value,
            ),
        });
        // Backend UI state merge cannot clear Option<String> with null.
        // Persist empty string as an explicit "go back to playlists" marker.
        void saveUiState({
            activePlaylistId: persisted.activePlaylistId ?? "",
            playlistScrollState: normalizedScrollState,
            playlistDrawerWidthRatio: normalizePlaylistDrawerWidthRatio(
                playlistDrawerWidthRatio.value,
            ),
        });
    };

    watch(
        () => activePanel.value,
        (panel) => {
            if (!hasLoadedPanel.value) return;
            void saveUiState({ activePanel: panel });
            schedulePointerRefresh();
        },
    );

    watch(
        () => activePlaylistId.value,
        () => {
            savePlaylistState();
        },
        { flush: "sync" },
    );

    watch(
        playlistDrawerWidthRatio,
        () => {
            savePlaylistState();
        },
        { deep: true },
    );

    watch(
        playlistScrollState,
        (scrollState) => {
            if (!hasLoadedPanel.value) return;
            const persisted = playlistState.toPersistedState();
            const normalizedScrollState = normalizePlaylistScrollState(scrollState);
            savePlaylistStateToLocal({
                activePlaylistId: persisted.activePlaylistId,
                playlistScrollState: normalizedScrollState,
                playlistDrawerWidthRatio: normalizePlaylistDrawerWidthRatio(
                    playlistDrawerWidthRatio.value,
                ),
            });
            scrollStateSaver.saveDebounced({
                playlistScrollState: normalizedScrollState,
            });
        },
        { deep: true },
    );

    onUnmounted(() => {
        scrollStateSaver.cancel();
    });

    return {
        hasLoadedPanel,
        loadActivePanel,
    };
};
