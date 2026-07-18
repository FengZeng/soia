import { onMounted, onUnmounted, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useAppEventBindings } from "./useAppEventBindings";

type UseAppEventBindingsOptions = Parameters<typeof useAppEventBindings>[0];
type RuntimeUiApi = UseAppEventBindingsOptions["ui"] & {
    stopInactivityTimer: () => void;
};

type NowPlayingApi = {
    clearNowPlaying: () => void;
    clearStatusOverlay: () => void;
    triggerStatusOverlayFromPlayback: () => void;
    updateNowPlayingMetadata: () => void;
};

type UseAppRuntimeBindingsOptions = Omit<UseAppEventBindingsOptions, "ui"> & {
    ui: RuntimeUiApi;
    nowPlaying: NowPlayingApi;
    isInfoOpen: Ref<boolean>;
    isPlaylistOpen: Ref<boolean>;
    hideHistory: Ref<boolean>;
    playerHeaderRef: Ref<{ $el?: HTMLElement } | null>;
    closePlaylist: () => void;
    shouldKeepControlsVisible: () => boolean;
    schedulePointerRefresh: () => void;
};

export const useAppRuntimeBindings = ({
    player,
    tracks,
    ui,
    onFullscreenTransition,
    onFullscreenTransitionEnd,
    onCloseAllMenus,
    onKeydown,
    onDoubleClick,
    setWindowControlsVisible,
    onFileLoaded,
    onPlaybackRestart,
    onProgress,
    onEndFile,
    resolveMediaTitle,
    nowPlaying,
    isInfoOpen,
    isPlaylistOpen,
    hideHistory,
    playerHeaderRef,
    closePlaylist,
    shouldKeepControlsVisible,
    schedulePointerRefresh,
}: UseAppRuntimeBindingsOptions) => {
    const isMacPlatform =
        typeof navigator !== "undefined" && /mac|darwin/i.test(navigator.userAgent);
    const setWindowVibrancyVisible = (visible: boolean) => {
        if (!isMacPlatform) return;
        void invoke("set_window_vibrancy_visible", { visible }).catch((error) => {
            console.warn("[windowVibrancy] Failed to toggle window vibrancy", {
                visible,
                error,
            });
        });
    };

    useAppEventBindings({
        player,
        tracks,
        ui,
        onFullscreenTransition,
        onFullscreenTransitionEnd,
        onCloseAllMenus,
        onKeydown,
        onDoubleClick,
        setWindowControlsVisible,
        onFileLoaded,
        onPlaybackRestart,
        onProgress,
        onEndFile,
        resolveMediaTitle,
    });

    watch(
        () => ui.showControls.value,
        (visible) => {
            void setWindowControlsVisible(visible);
            if (!visible && !shouldKeepControlsVisible()) {
                isInfoOpen.value = false;
                closePlaylist();
            }
        },
    );

    watch(
        () => player.state.media.isFileLoaded,
        (loaded) => {
            setWindowVibrancyVisible(!loaded);
            if (!loaded) {
                isInfoOpen.value = false;
                nowPlaying.clearNowPlaying();
                ui.stopInactivityTimer();
                void setWindowControlsVisible(true);
                nowPlaying.clearStatusOverlay();
            }
            schedulePointerRefresh();
        },
        { immediate: true },
    );

    watch(
        () => player.state.playback.isPlaying,
        () => {
            if (!player.state.media.isFileLoaded) return;
            nowPlaying.triggerStatusOverlayFromPlayback();
        },
    );

    watch(
        () => player.state.playback.duration,
        () => {
            if (!player.state.media.isFileLoaded) return;
            nowPlaying.updateNowPlayingMetadata();
        },
    );

    watch([hideHistory, isPlaylistOpen], () => {
        schedulePointerRefresh();
    });

    const handleGlobalPointerDown = (event: MouseEvent) => {
        if (!isInfoOpen.value) return;
        const headerEl = playerHeaderRef.value?.$el as HTMLElement | undefined;
        if (headerEl && !headerEl.contains(event.target as Node)) {
            isInfoOpen.value = false;
        }
    };

    onMounted(() => {
        window.addEventListener("mousedown", handleGlobalPointerDown);
    });

    onUnmounted(() => {
        window.removeEventListener("mousedown", handleGlobalPointerDown);
    });
};
