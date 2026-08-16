import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { PlayerApi } from "./usePlaybackController";

type PlaybackOverlayOptions = {
    player: PlayerApi;
    isLoading: { value: boolean };
};

const formatSeekDeltaSeconds = (delta: number) => {
    const seconds = Math.abs(delta);
    const rounded = Math.round(seconds * 10) / 10;
    return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
};

const clampNumber = (value: number, min: number, max: number) =>
    Math.min(Math.max(value, min), max);

export const usePlaybackOverlays = ({
    player,
    isLoading,
}: PlaybackOverlayOptions) => {
    const shouldShowPlaybackLoadingOverlay = computed(
        () =>
            isLoading.value ||
            (player.state.media.isFileLoaded && player.state.playback.isBuffering),
    );
    const loadingDownloadSpeedBps = computed(() => {
        const speed = player.state.playback.downloadSpeedBps;
        if (!Number.isFinite(speed) || speed <= 0) return null;
        return speed;
    });
    const isLoadingOverlayVisible = ref(false);
    const seekOverlayLeftText = ref("");
    const seekOverlayRightText = ref("");
    const seekOverlayLeftTimelineText = ref("");
    const volumeOverlayText = ref("");
    // label: e.g. "Zoom" or "Playback", value: e.g. "1.2x"
    const zoomOverlayText = ref<{ label: string; value: string } | null>(null);
    const seekOverlayLeftPulseToken = ref(0);
    const seekOverlayRightPulseToken = ref(0);
    let loadingOverlayDelayTimer: ReturnType<typeof setTimeout> | null = null;
    let seekOverlayTimer: ReturnType<typeof setTimeout> | null = null;
    let volumeOverlayTimer: ReturnType<typeof setTimeout> | null = null;
    let zoomOverlayTimer: ReturnType<typeof setTimeout> | null = null;
    let seekOverlayAccumulatedDelta = 0;
    let seekOverlayBaseTime = 0;

    const clearLoadingOverlayDelayTimer = () => {
        if (loadingOverlayDelayTimer !== null) {
            window.clearTimeout(loadingOverlayDelayTimer);
            loadingOverlayDelayTimer = null;
        }
    };

    const clearSeekOverlayTimer = () => {
        if (seekOverlayTimer !== null) {
            window.clearTimeout(seekOverlayTimer);
            seekOverlayTimer = null;
        }
    };

    const clearVolumeOverlayTimer = () => {
        if (volumeOverlayTimer !== null) {
            window.clearTimeout(volumeOverlayTimer);
            volumeOverlayTimer = null;
        }
    };

    const buildSeekTimelineText = (targetTime: number, duration: number) => {
        const safeDuration =
            Number.isFinite(duration) && duration > 0 ? duration : 0;
        const safeTarget = clampNumber(targetTime, 0, safeDuration);
        return `${player.formatTime(safeTarget)} / ${player.formatTime(safeDuration)}`;
    };

    const showSeekOverlay = (delta: number) => {
        const hasActiveOverlay = seekOverlayTimer !== null;
        const hasSameDirection =
            seekOverlayAccumulatedDelta === 0 ||
            Math.sign(seekOverlayAccumulatedDelta) === Math.sign(delta);
        if (!(hasActiveOverlay && hasSameDirection)) {
            seekOverlayBaseTime = player.state.playback.currentTime;
        }
        seekOverlayAccumulatedDelta =
            hasActiveOverlay && hasSameDirection
                ? seekOverlayAccumulatedDelta + delta
                : delta;
        clearSeekOverlayTimer();
        seekOverlayLeftText.value = "";
        seekOverlayRightText.value = "";
        seekOverlayLeftTimelineText.value = "";

        const seekTargetTime = seekOverlayBaseTime + seekOverlayAccumulatedDelta;
        const timelineText = buildSeekTimelineText(
            seekTargetTime,
            player.state.playback.duration,
        );
        if (seekOverlayAccumulatedDelta < 0) {
            seekOverlayLeftText.value = `- ${formatSeekDeltaSeconds(
                seekOverlayAccumulatedDelta,
            )}`;
            seekOverlayLeftTimelineText.value = timelineText;
            seekOverlayLeftPulseToken.value += 1;
        } else if (seekOverlayAccumulatedDelta > 0) {
            seekOverlayRightText.value = `+ ${formatSeekDeltaSeconds(
                seekOverlayAccumulatedDelta,
            )}`;
            seekOverlayLeftTimelineText.value = timelineText;
            seekOverlayRightPulseToken.value += 1;
        }
        seekOverlayTimer = window.setTimeout(() => {
            seekOverlayLeftText.value = "";
            seekOverlayRightText.value = "";
            seekOverlayLeftTimelineText.value = "";
            seekOverlayTimer = null;
            seekOverlayAccumulatedDelta = 0;
            seekOverlayBaseTime = 0;
        }, 700);
    };

    const showVolumeOverlay = (volume: number) => {
        const nextVolume = clampNumber(Math.round(volume), 0, 100);
        volumeOverlayText.value = `Volume ${nextVolume}`;
        clearVolumeOverlayTimer();
        volumeOverlayTimer = window.setTimeout(() => {
            volumeOverlayText.value = "";
            volumeOverlayTimer = null;
        }, 700);
    };

    const clearZoomOverlayTimer = () => {
        if (zoomOverlayTimer !== null) {
            window.clearTimeout(zoomOverlayTimer);
            zoomOverlayTimer = null;
        }
    };

    const formatScaleValue = (scale: number): string => {
        const rounded = Math.round(scale * 100) / 100;
        return `${rounded.toFixed(rounded % 1 === 0 ? 0 : 1)}x`;
    };

    const showZoomOverlay = (scale: number) => {
        zoomOverlayText.value = { label: "Zoom", value: formatScaleValue(scale) };
        clearZoomOverlayTimer();
        zoomOverlayTimer = window.setTimeout(() => {
            zoomOverlayText.value = null;
            zoomOverlayTimer = null;
        }, 1200);
    };

    watch(shouldShowPlaybackLoadingOverlay, (shouldShow) => {
        if (!shouldShow) {
            clearLoadingOverlayDelayTimer();
            isLoadingOverlayVisible.value = false;
            return;
        }
        if (isLoadingOverlayVisible.value) return;
        clearLoadingOverlayDelayTimer();
        loadingOverlayDelayTimer = window.setTimeout(() => {
            loadingOverlayDelayTimer = null;
            isLoadingOverlayVisible.value = true;
        }, 500);
    });

    onBeforeUnmount(() => {
        clearLoadingOverlayDelayTimer();
        clearSeekOverlayTimer();
        clearVolumeOverlayTimer();
        clearZoomOverlayTimer();
    });

    return {
        isLoadingOverlayVisible,
        loadingDownloadSpeedBps,
        seekOverlayLeftText,
        seekOverlayRightText,
        seekOverlayLeftTimelineText,
        volumeOverlayText,
        zoomOverlayText,
        seekOverlayLeftPulseToken,
        seekOverlayRightPulseToken,
        showSeekOverlay,
        showVolumeOverlay,
        showZoomOverlay,
    };
};
