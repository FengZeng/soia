import { invoke } from "@tauri-apps/api/core";
import { onUnmounted, ref } from "vue";
import { getPathDisplayName } from "../utils/getPathDisplayName";

type UseNowPlayingStateOptions = {
    isFileLoaded: () => boolean;
    mediaUrl: () => string;
    mediaTitle: () => string;
    isPlaying: () => boolean;
    duration: () => number;
};

const STATUS_OVERLAY_MS = 1000;

export const useNowPlayingState = ({
    isFileLoaded,
    mediaUrl,
    mediaTitle,
    isPlaying,
    duration,
}: UseNowPlayingStateOptions) => {
    const nowPlayingArtworkPath = ref("");
    const showStatusOverlay = ref(false);
    const statusOverlayMode = ref<"play" | "pause">("play");
    let statusOverlayTimer: number | null = null;

    const clearArtwork = () => {
        nowPlayingArtworkPath.value = "";
    };

    const clearStatusOverlay = () => {
        showStatusOverlay.value = false;
        if (statusOverlayTimer) {
            window.clearTimeout(statusOverlayTimer);
            statusOverlayTimer = null;
        }
    };

    const triggerStatusOverlay = (mode: "play" | "pause") => {
        statusOverlayMode.value = mode;
        showStatusOverlay.value = true;
        if (statusOverlayTimer) {
            window.clearTimeout(statusOverlayTimer);
        }
        statusOverlayTimer = window.setTimeout(() => {
            showStatusOverlay.value = false;
            statusOverlayTimer = null;
        }, STATUS_OVERLAY_MS);
    };

    const triggerStatusOverlayFromPlayback = () => {
        triggerStatusOverlay(isPlaying() ? "play" : "pause");
    };

    const updateNowPlayingMetadata = () => {
        if (!isFileLoaded()) return;
        const mediaDuration = duration();
        const title = mediaTitle().trim() || getPathDisplayName(mediaUrl());
        void invoke("set_now_playing_metadata", {
            title,
            duration: mediaDuration > 0 ? mediaDuration : null,
            artworkPath: nowPlayingArtworkPath.value || null,
        });
    };

    const clearNowPlaying = () => {
        clearArtwork();
        void invoke("clear_now_playing");
    };

    const captureNowPlayingArtwork = async () => {
        if (!isFileLoaded()) return;
        const path = await invoke<string | null>("capture_now_playing_artwork", {
            url: mediaUrl(),
        });
        if (!path) return;
        nowPlayingArtworkPath.value = path;
        updateNowPlayingMetadata();
    };

    onUnmounted(() => {
        clearStatusOverlay();
    });

    return {
        nowPlayingArtworkPath,
        showStatusOverlay,
        statusOverlayMode,
        clearArtwork,
        clearStatusOverlay,
        triggerStatusOverlayFromPlayback,
        updateNowPlayingMetadata,
        clearNowPlaying,
        captureNowPlayingArtwork,
    };
};
