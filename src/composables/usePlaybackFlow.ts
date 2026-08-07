import { computed, onMounted, onUnmounted, ref, type Ref } from "vue";
import type { HistoryEntry } from "../types/history";
import type { NetworkPlayRequest } from "../types/network";
import type { PlaylistSourceClient } from "../core-client/PlaylistSourceClient";
import type { PlayerApi } from "./usePlaybackController";
import { loadUiState } from "./useUiStateStore";
import { isLikelyLivePlaybackSource } from "../utils/livePlayback";
import {
    ALLOW_URL_INPUT_DURING_PLAYBACK_SETTING_LABEL,
    DISABLE_SUBTITLES_SETTING_LABEL,
    ENABLE_COMPACT_MODE_SETTING_LABEL,
    PLAYBACK_TITLE_SETTING_LABEL,
    SETTINGS_UPDATED_EVENT,
    WALLPAPER_MODE_SETTING_LABEL,
    type PlaybackTitleMode,
} from "../mock/settings";

type TracksApi = {
    resetTracks: () => void;
};

type HistoryApi = {
    recordStop: (
        path: string,
        position: number,
        duration: number,
        title?: string,
        isLivePlayback?: boolean,
    ) => Promise<void>;
    updateTitle: (path: string, title: string) => void;
};

type PlaybackRequestOptions = {
    isLivePlayback?: boolean;
};

type PlaylistCreationConfirmation = {
    shouldCreate: boolean;
    name: string;
};

type NowPlayingApi = {
    clearArtwork: () => void;
    clearNowPlaying: () => void;
};

type UsePlaybackFlowOptions = {
    isMacOS: boolean;
    player: PlayerApi;
    playlistSourceClient: PlaylistSourceClient;
    tracks: TracksApi;
    history: HistoryApi;
    nowPlaying: NowPlayingApi;
    hideAllMenus: () => void;
    isInfoOpen: Ref<boolean>;
    loadingState?: {
        isLoading: Ref<boolean>;
        loadingUrl: Ref<string>;
    };
    onPlaybackIntent?: () => void | Promise<void>;
    requestPlaylistCreation?: (request: {
        defaultName: string;
        itemCount: number;
        sourceLabel?: string;
    }) => Promise<PlaylistCreationConfirmation>;
    onPlaylistCreated?: (playlistId: string) => void;
};

type StoredSettingGroup = {
    title: string;
    items: Array<{ label: string; value: string }>;
};

type PlaybackPreferences = {
    playbackTitleMode: PlaybackTitleMode;
    compactModeEnabled: boolean;
    wallpaperModeEnabled: boolean;
    subtitlesDisabled: boolean;
};

const DEFAULT_PLAYBACK_PREFERENCES: PlaybackPreferences = {
    playbackTitleMode: "Show",
    compactModeEnabled: false,
    wallpaperModeEnabled: false,
    subtitlesDisabled: false,
};
const SINGLE_ENTRY_PLAYLIST_VOD_DURATION_SECONDS = 5 * 60;

const normalizePlaybackTitleMode = (
    value?: string | null,
): PlaybackTitleMode => {
    const normalized = value?.trim().toLowerCase();
    if (normalized === "editable" || normalized === "on") {
        return "Editable";
    }
    if (normalized === "hidden") {
        return "Hidden";
    }
    return "Show";
};

const parsePlaybackPreferences = (
    groups?: StoredSettingGroup[],
): PlaybackPreferences => {
    const items = groups?.flatMap((group) => group.items) ?? [];
    const getValue = (label: string) =>
        items.find((item) => item.label === label)?.value;

    const playbackTitleModeValue = normalizePlaybackTitleMode(
        getValue(PLAYBACK_TITLE_SETTING_LABEL) ??
            getValue(ALLOW_URL_INPUT_DURING_PLAYBACK_SETTING_LABEL),
    );
    const compactModeValue = getValue(ENABLE_COMPACT_MODE_SETTING_LABEL) ?? "On";
    const wallpaperModeValue = getValue(WALLPAPER_MODE_SETTING_LABEL) ?? "Disable";
    const subtitlesDisabledValue =
        getValue(DISABLE_SUBTITLES_SETTING_LABEL) ?? "Off";

    return {
        playbackTitleMode: playbackTitleModeValue,
        compactModeEnabled: compactModeValue === "On",
        wallpaperModeEnabled: wallpaperModeValue === "Enable",
        subtitlesDisabled: subtitlesDisabledValue === "On",
    };
};

export const usePlaybackFlow = ({
    isMacOS,
    player,
    playlistSourceClient,
    tracks,
    history,
    nowPlaying,
    hideAllMenus,
    isInfoOpen,
    loadingState,
    onPlaybackIntent,
    requestPlaylistCreation,
    onPlaylistCreated,
}: UsePlaybackFlowOptions) => {
    const isLoading = loadingState?.isLoading ?? ref(false);
    const loadingUrl = loadingState?.loadingUrl ?? ref("");
    const pendingResume = ref<{ url: string; position: number } | null>(null);
    const hideHistory = ref(false);
    const playbackPreferences = ref<PlaybackPreferences>({
        ...DEFAULT_PLAYBACK_PREFERENCES,
        compactModeEnabled: true,
        wallpaperModeEnabled: false,
    });
    const preferredTitleByUrl = new Map<string, string>();
    const preferredTitleByResourceKey = new Map<string, string>();
    const livePlaybackKeys = new Set<string>();
    const nonLivePlaybackKeys = new Set<string>();
    const livePlaybackPlaylistEntryCounts = new Map<string, number>();
    const playlistSourceClientId = typeof crypto !== "undefined" && crypto.randomUUID
        ? `desktop-playlist-source-${crypto.randomUUID()}`
        : `desktop-playlist-source-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

    const updatePlaybackPreferences = (groups?: StoredSettingGroup[]) => {
        playbackPreferences.value = parsePlaybackPreferences(groups);
    };

    const loadPlaybackPreferences = async () => {
        const stored = await loadUiState<{
            settings?: {
                groups?: StoredSettingGroup[];
            };
        }>();
        updatePlaybackPreferences(stored?.settings?.groups);
    };

    const onSettingsUpdated = (event: Event) => {
        const customEvent = event as CustomEvent<{ groups?: StoredSettingGroup[] }>;
        updatePlaybackPreferences(customEvent.detail?.groups);
    };

    const triggerPlaybackIntent = async () => {
        await onPlaybackIntent?.();
    };

    const resetPlaybackTimeline = () => {
        player.state.media.isFileLoaded = false;
        player.state.playback.currentTime = 0;
        player.state.playback.duration = 0;
        player.state.playback.bufferedTime = 0;
    };

    const rememberNonLivePlaybackSource = (path: string) => {
        if (!path) return;
        nonLivePlaybackKeys.add(path);
        livePlaybackKeys.delete(path);
        livePlaybackPlaylistEntryCounts.delete(path);
    };

    const updateLivePlaybackForDuration = (duration: number) => {
        if (!player.state.media.isLivePlayback) return;
        if (
            !Number.isFinite(duration) ||
            duration <= SINGLE_ENTRY_PLAYLIST_VOD_DURATION_SECONDS
        ) {
            return;
        }
        const playbackKey = player.state.media.url;
        if (livePlaybackPlaylistEntryCounts.get(playbackKey) !== 1) return;
        player.state.media.isLivePlayback = false;
        rememberNonLivePlaybackSource(playbackKey);
    };

    const shouldTreatAsLivePlayback = (
        playbackKey: string,
        options?: PlaybackRequestOptions,
    ) => {
        if (typeof options?.isLivePlayback === "boolean") {
            return options.isLivePlayback;
        }
        if (nonLivePlaybackKeys.has(playbackKey)) return false;
        return (
            livePlaybackKeys.has(playbackKey) ||
            isLikelyLivePlaybackSource(playbackKey)
        );
    };

    const resourceKeyFromUrl = (value: string) => {
        const raw = value.trim();
        if (!raw) return "";
        try {
            const parsed = new URL(raw);
            const pathname = decodeURIComponent(parsed.pathname || "").trim();
            if (!pathname) return parsed.origin.toLowerCase();
            const baseKey = `${parsed.origin}${pathname}`.toLowerCase();
            const hostname = parsed.hostname.toLowerCase();
            const isYouTubeHost =
                hostname === "youtube.com" ||
                hostname === "www.youtube.com" ||
                hostname === "music.youtube.com";
            if (isYouTubeHost) {
                // YouTube identifies videos and playlists in the query string.
                // Dropping it aliases every `/watch` URL to the same preferred
                // title cache entry, causing next/previous to reuse the old title.
                const identity = ["v", "list"]
                    .map((name) => [name, parsed.searchParams.get(name)?.trim()] as const)
                    .filter((entry): entry is readonly [string, string] => !!entry[1])
                    .map(([name, queryValue]) =>
                        `${name}=${encodeURIComponent(queryValue)}`,
                    )
                    .join("&");
                if (identity) return `${baseKey}?${identity}`;
            }
            return baseKey;
        } catch {
            return raw.toLowerCase();
        }
    };

    const rememberPreferredTitle = (url: string, preferredTitle?: string) => {
        const normalizedPreferredTitle = preferredTitle?.trim() || "";
        if (!normalizedPreferredTitle) return "";
        const fileNameFromUrl = (() => {
            try {
                const parsed = new URL(url);
                const pathname = decodeURIComponent(parsed.pathname || "");
                const segments = pathname.split("/").filter(Boolean);
                return segments.length ? segments[segments.length - 1] : "";
            } catch {
                const segments = url.split("/").filter(Boolean);
                return segments.length ? segments[segments.length - 1] : "";
            }
        })();
        const extensionMatch = fileNameFromUrl.match(/(\.[a-z0-9]{1,8})$/i);
        const normalizedWithExtension =
            !/\.[a-z0-9]{1,8}$/i.test(normalizedPreferredTitle) && extensionMatch
                ? `${normalizedPreferredTitle}${extensionMatch[1]}`
                : normalizedPreferredTitle;
        preferredTitleByUrl.set(url, normalizedWithExtension);
        const key = resourceKeyFromUrl(url);
        if (key) preferredTitleByResourceKey.set(key, normalizedWithExtension);
        return normalizedWithExtension;
    };

    const applyResolvedMediaTitle = (url: string, title?: string | null) => {
        const normalizedTitle = title?.trim() || "";
        if (!normalizedTitle) return;
        if (player.state.media.url !== url) return;
        player.state.media.title = rememberPreferredTitle(url, normalizedTitle);
        history.updateTitle(url, player.state.media.title);
    };

    const playSource = async (
        keyOrUrl: string,
        preferredTitle?: string,
        options?: PlaybackRequestOptions,
    ) => {
        const requestedKey = keyOrUrl.trim();
        if (!requestedKey) return;
        await triggerPlaybackIntent();
        resetPlaybackTimeline();
        hideHistory.value = true;
        nowPlaying.clearArtwork();
        tracks.resetTracks();
        player.state.media.url = requestedKey;
        player.state.media.isLivePlayback = shouldTreatAsLivePlayback(
            requestedKey,
            options,
        );
        player.state.media.title = rememberPreferredTitle(
            requestedKey,
            preferredTitle,
        );
        player.state.playback.isBuffering = false;
        player.state.playback.downloadSpeedBps = 0;
        player.state.playback.hwdecCurrent = "";
        loadingUrl.value = requestedKey;
        isLoading.value = true;
        const result = await player.loadPlaybackSource(
            requestedKey,
            preferredTitle?.trim() || undefined,
        );
        if (result.superseded) return;
        const playbackKey = result.playbackKey?.trim() || requestedKey;
        player.state.media.url = playbackKey;
        if (isLoading.value) {
            loadingUrl.value = playbackKey;
        }
        if (preferredTitle?.trim()) {
            player.state.media.title = rememberPreferredTitle(
                playbackKey,
                preferredTitle,
            );
        }
        if (result.isLivePlayback) {
            player.state.media.isLivePlayback = true;
        }
        applyResolvedMediaTitle(playbackKey, result.title);
    };

    const playPath = async (
        path: string,
        preferredTitle?: string,
        options?: PlaybackRequestOptions,
    ) => {
        if (!path) return;
        await playSource(path, preferredTitle, options);
    };

    const confirmPlaylistCreation = async (
        defaultName: string,
        itemCount: number,
        sourceLabel?: string,
    ): Promise<PlaylistCreationConfirmation> => {
        if (!requestPlaylistCreation) {
            return { shouldCreate: true, name: defaultName };
        }
        return requestPlaylistCreation({ defaultName, itemCount, sourceLabel });
    };

    const continuePlaylistSourceOperation = async (
        action: { operationId: string; suggestedName: string; itemCount: number; sourceLabel: string | null },
    ) => {
        const confirmation = await confirmPlaylistCreation(
            action.suggestedName,
            action.itemCount,
            action.sourceLabel ?? undefined,
        );
        const result = await playlistSourceClient.continue({
            clientId: playlistSourceClientId,
            operationId: action.operationId,
            createPlaylist: confirmation.shouldCreate,
            playlistName: confirmation.shouldCreate ? confirmation.name : null,
        });
        if (result.playlistId) onPlaylistCreated?.(result.playlistId);
        return result;
    };

    const preparePlaylistSourceOperation = async (
        sources: string[],
        preferredTitle?: string,
    ) => {
        const requestedKey = sources.length === 1 ? sources[0]?.trim() || "" : "";
        await triggerPlaybackIntent();
        resetPlaybackTimeline();
        hideHistory.value = true;
        nowPlaying.clearArtwork();
        tracks.resetTracks();
        if (requestedKey) {
            player.state.media.url = requestedKey;
            player.state.media.isLivePlayback = shouldTreatAsLivePlayback(requestedKey);
            player.state.media.title = rememberPreferredTitle(
                requestedKey,
                preferredTitle,
            );
            loadingUrl.value = requestedKey;
            isLoading.value = true;
        }
        const prepared = await playlistSourceClient.prepare({
            clientId: playlistSourceClientId,
            sources,
            preferredTitle: preferredTitle ?? null,
        });
        const result = prepared.type === "clientActionRequired"
            ? await continuePlaylistSourceOperation(prepared.action)
            : prepared.result;
        if (result.superseded) return result;
        const playbackKey = result.playbackKey?.trim() || sources[0]?.trim() || "";
        const isLivePlayback = prepared.type === "clientActionRequired"
            ? prepared.isLivePlayback ?? result.isLivePlayback
            : result.isLivePlayback;
        const playlistEntryCount = prepared.type === "clientActionRequired"
            ? prepared.playlistEntryCount
            : prepared.playlistEntryCount;
        if (playbackKey) {
            player.state.media.url = playbackKey;
            if (isLivePlayback) {
                livePlaybackKeys.add(playbackKey);
                nonLivePlaybackKeys.delete(playbackKey);
                if (playlistEntryCount !== null) {
                    livePlaybackPlaylistEntryCounts.set(
                        playbackKey,
                        playlistEntryCount,
                    );
                } else {
                    livePlaybackPlaylistEntryCounts.delete(playbackKey);
                }
            } else {
                rememberNonLivePlaybackSource(playbackKey);
            }
        }
        player.state.media.isLivePlayback = isLivePlayback;
        if (preferredTitle?.trim() && playbackKey) {
            player.state.media.title = rememberPreferredTitle(
                playbackKey,
                preferredTitle,
            );
        }
        if (playbackKey) applyResolvedMediaTitle(playbackKey, result.title);
        return result;
    };

    const openWithSelected = async (selected: string[]) => {
        if (!selected.length) {
            hideHistory.value = false;
            isLoading.value = false;
            return;
        }
        await preparePlaylistSourceOperation(selected);
    };

    const openWithFilePicker = async () => {
        hideHistory.value = true;
        const selected = await player.pickFiles();
        await openWithSelected(selected);
    };

    const openWithAutoPicker = async () => {
        hideHistory.value = true;
        const selected = await player.pickMediaPathsAuto();
        await openWithSelected(selected);
    };

    const requestOpenFilePicker = () => {
        if (isMacOS) {
            void openWithAutoPicker();
            return;
        }
        void openWithFilePicker();
    };

    const onLoadFile = async () => {
        if (!player.state.media.url) return;
        await preparePlaylistSourceOperation([player.state.media.url]);
        if (!player.state.media.isFileLoaded) {
            hideHistory.value = false;
        }
    };

    onMounted(() => {
        void loadPlaybackPreferences();
        if (typeof window === "undefined") return;
        window.addEventListener(SETTINGS_UPDATED_EVENT, onSettingsUpdated);
    });

    onUnmounted(() => {
        if (typeof window === "undefined") return;
        window.removeEventListener(SETTINGS_UPDATED_EVENT, onSettingsUpdated);
    });

    const onPlayHistory = async (entry: HistoryEntry) => {
        const preferredTitle = entry.title?.trim() || "";
        await preparePlaylistSourceOperation([entry.path], preferredTitle);
    };

    const onPlayNetwork = async (payload: NetworkPlayRequest) => {
        const displayName = payload.displayName?.trim() || "";
        await preparePlaylistSourceOperation([payload.playbackKey], displayName);
    };

    const onUpdateUrl = (value: string) => {
        player.state.media.url = value;
        player.state.media.title = "";
        player.state.media.isLivePlayback = false;
        isLoading.value = false;
        loadingUrl.value = "";
        player.state.playback.isBuffering = false;
        player.state.playback.downloadSpeedBps = 0;
        player.state.playback.hwdecCurrent = "";
        nowPlaying.clearArtwork();
    };

    const resolveMediaTitle = (incomingTitle: string, currentUrl: string) => {
        const preferred = preferredTitleByUrl.get(currentUrl)?.trim() || "";
        if (preferred) return preferred;
        const preferredByKey =
            preferredTitleByResourceKey
                .get(resourceKeyFromUrl(currentUrl))
                ?.trim() || "";
        if (preferredByKey) return preferredByKey;
        return incomingTitle.trim();
    };

    const onStopPlayback = async () => {
        isLoading.value = false;
        loadingUrl.value = "";
        nowPlaying.clearNowPlaying();
        await history.recordStop(
            player.state.media.url,
            player.state.playback.currentTime,
            player.state.playback.duration,
            player.state.media.title,
            player.state.media.isLivePlayback,
        );
        await player.stopPlayback();
        hideHistory.value = false;
        player.state.media.isFileLoaded = false;
        player.state.media.isLivePlayback = false;
        player.state.media.title = "";
        player.state.playback.isPlaying = false;
        player.state.playback.isBuffering = false;
        player.state.playback.downloadSpeedBps = 0;
        player.state.playback.currentTime = 0;
        player.state.playback.duration = 0;
        player.state.playback.bufferedTime = 0;
        player.state.playback.videoBitrate = 0;
        player.state.playback.hwdecCurrent = "";
        tracks.resetTracks();
        hideAllMenus();
        isInfoOpen.value = false;
    };

    const isLoadingForCurrentUrl = computed(
        () => isLoading.value && loadingUrl.value === player.state.media.url,
    );
    const playbackTitleMode = computed(
        () => playbackPreferences.value.playbackTitleMode,
    );
    const compactModeEnabled = computed(
        () => playbackPreferences.value.compactModeEnabled,
    );
    const wallpaperModeEnabled = computed(
        () => playbackPreferences.value.wallpaperModeEnabled,
    );
    const subtitlesDisabled = computed(
        () => playbackPreferences.value.subtitlesDisabled,
    );

    return {
        isLoading,
        loadingUrl,
        pendingResume,
        hideHistory,
        isLoadingForCurrentUrl,
        playPath,
        onLoadFile,
        onPlayHistory,
        onPlayNetwork,
        onUpdateUrl,
        updateLivePlaybackForDuration,
        resolveMediaTitle,
        onStopPlayback,
        requestOpenFilePicker,
        openSelectedPaths: openWithSelected,
        playbackTitleMode,
        compactModeEnabled,
        wallpaperModeEnabled,
        subtitlesDisabled,
    };
};
