import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { usePlaybackController } from "./usePlaybackController";
import { useMediaTracks } from "./useMediaTracks";
import { usePlaybackSpeed } from "./usePlaybackSpeed";
import { usePlaybackAdjustments } from "./usePlaybackAdjustments";
import { useSubtitleAppearance } from "./useSubtitleAppearance";
import { usePlaybackHistory } from "./usePlaybackHistory";
import { usePlaylistState } from "./usePlaylistState";
import { usePointerUiState } from "./usePointerUiState";
import { useMenuControls } from "./useMenuControls";
import { useNowPlayingState } from "./useNowPlayingState";
import { useUiControls } from "./useUiControls";
import { useFullscreenTransition } from "./useFullscreenTransition";
import type { CoreClient } from "../core-client/CoreClient";

export type SideActionId = "home" | "history" | "network" | "settings";
export type ClearConfirmTarget = "playlist" | "history" | null;

const detectMacOS = () =>
    typeof navigator !== "undefined" && /mac|darwin/i.test(navigator.userAgent);

const detectNativePipPlatform = () =>
    typeof navigator !== "undefined" &&
    /\b(mac|darwin|windows)\b/i.test(navigator.userAgent);

export const useAppBootstrap = (coreClient: CoreClient) => {
    const isMacOS = detectMacOS();
    const isNativePipPlatform = detectNativePipPlatform();
    const player = usePlaybackController(coreClient);
    const history = usePlaybackHistory();
    const tracks = useMediaTracks(() => player.state.media.url, history);
    const speed = usePlaybackSpeed(coreClient);
    const adjustments = usePlaybackAdjustments();
    const subtitleAppearance = useSubtitleAppearance();
    const playlistState = usePlaylistState();

    const {
        playlists,
        activePlaylistId,
        activePlaylist,
        playlist,
        loopMode,
        sortMode,
        isLoopOne,
        orderedPlaylist,
    } = playlistState;
    const isInfoOpen = ref(false);
    const isPlaylistOpen = ref(false);
    const activePanel = ref<SideActionId>("home");
    const clearConfirmTarget = ref<ClearConfirmTarget>(null);

    const pointerUi = usePointerUiState();
    const isPointerOverUi = pointerUi.isPointerOverUi;
    const isPointerNearLeft = pointerUi.isPointerNearLeft;
    const schedulePointerRefresh = pointerUi.schedulePointerRefresh;

    const isPipEnabled = ref(false);
    let unlistenNativePipChanged: UnlistenFn | null = null;

    const refreshPipState = async () => {
        if (!isNativePipPlatform) return;
        try {
            isPipEnabled.value = await invoke<boolean>("is_native_pip_enabled");
        } catch {
            // Ignore in non-Tauri environments.
        }
    };

    const shouldKeepControlsVisible = () =>
        isPointerOverUi.value || (isMacOS && isPipEnabled.value);

    const { hideAllMenus, toggleMenu, closeAllMenus } = useMenuControls(
        tracks,
        speed,
        adjustments,
    );

    const {
        isFullscreenTransitioning,
        triggerFullscreenTransition,
        resetFullscreenTransition,
        onToggleFullscreen,
    } = useFullscreenTransition(player.toggleFullscreen);

    const playerHeaderRef = ref<{ $el?: HTMLElement } | null>(null);

    const nowPlaying = useNowPlayingState({
        isFileLoaded: () => player.state.media.isFileLoaded,
        mediaUrl: () => player.state.media.url,
        mediaTitle: () => player.state.media.title,
        isPlaying: () => player.state.playback.isPlaying,
        duration: () => player.state.playback.duration,
    });

    const ui = useUiControls(
        () => player.state.media.isFileLoaded,
        hideAllMenus,
        shouldKeepControlsVisible,
        schedulePointerRefresh,
    );

    watch(isPipEnabled, (enabled) => {
        if (!isMacOS || !enabled) return;
        ui.showControls.value = true;
    });

    const isPlaybackActive = computed(() => player.state.media.isFileLoaded);
    const navActivePanel = computed<SideActionId | null>(() =>
        isPlaybackActive.value ? null : activePanel.value,
    );
    const hasAudioTracks = computed(() => tracks.audioTracks.value.length > 0);
    const hasSubTracks = computed(() =>
        tracks.subTracks.value.some((track) => track.id !== 0),
    );

    const setWindowControlsVisible = async (visible: boolean) => {
        try {
            await invoke("set_window_controls_visible", { visible });
        } catch {
            // no-op on non-macOS or if unavailable
        }
    };

    const normalizeStoredPanel = (panel: string): SideActionId => {
        if (panel === "setting") return "settings";
        if (panel === "home") return "home";
        if (panel === "history") return "history";
        if (panel === "network") return "network";
        return "settings";
    };

    onMounted(async () => {
        await refreshPipState();
        if (!isNativePipPlatform) return;
        try {
            unlistenNativePipChanged = await listen<boolean>(
                "native-pip-changed",
                (event) => {
                    isPipEnabled.value = Boolean(event.payload);
                },
            );
        } catch {
            // Ignore in non-Tauri environments.
        }
    });

    onUnmounted(() => {
        unlistenNativePipChanged?.();
        unlistenNativePipChanged = null;
    });

    return {
        isMacOS,
        player,
        tracks,
        speed,
        adjustments,
        subtitleAppearance,
        history,
        playlistState,
        playlists,
        activePlaylistId,
        activePlaylist,
        playlist,
        loopMode,
        sortMode,
        isLoopOne,
        orderedPlaylist,
        isInfoOpen,
        isPlaylistOpen,
        activePanel,
        clearConfirmTarget,
        isPointerOverUi,
        isPointerNearLeft,
        isPipEnabled,
        schedulePointerRefresh,
        shouldKeepControlsVisible,
        hideAllMenus,
        toggleMenu,
        closeAllMenus,
        isFullscreenTransitioning,
        triggerFullscreenTransition,
        resetFullscreenTransition,
        onToggleFullscreen,
        playerHeaderRef,
        nowPlaying,
        ui,
        isPlaybackActive,
        navActivePanel,
        hasAudioTracks,
        hasSubTracks,
        setWindowControlsVisible,
        normalizeStoredPanel,
    };
};
