import { onMounted, onUnmounted, ref } from "vue";
import {
    SEEK_STEP_SETTING_LABEL,
    SETTINGS_UPDATED_EVENT,
} from "../mock/settings";
import { loadUiState } from "./useUiStateStore";
import { useSurroundSound, type SurroundPreset } from "./useSurroundSound";

type PlaybackShortcutApi = {
    state: {
        media: {
            isFileLoaded: boolean;
            isLivePlayback: boolean;
        };
        playback: {
            duration: number;
            volume: number;
        };
        window: {
            isFullscreen: boolean;
        };
    };
    togglePlayPause: () => Promise<void>;
    seekRelative: (position: number) => Promise<void>;
    setVolume: (volume: number) => Promise<void>;
};

type StoredSettingGroup = {
    title: string;
    items: Array<{ label: string; value: string }>;
};

const DEFAULT_SEEK_STEP_SECONDS = 5;
const VOLUME_STEP = 5;
const ZOOM_STEP = 0.1;
const ZOOM_MIN = 0.1;
const ZOOM_MAX = 4.0;
const SPEED_STEP = 0.1;
const SPEED_MIN = 0.1;
const SPEED_MAX = 4.0;

const parseSeekStepSeconds = (groups?: StoredSettingGroup[]): number => {
    const rawValue = groups
        ?.flatMap((group) => group.items)
        .find((item) => item.label === SEEK_STEP_SETTING_LABEL)?.value;
    const parsed = Number.parseFloat(rawValue ?? "");
    if (!Number.isFinite(parsed) || parsed <= 0) return DEFAULT_SEEK_STEP_SECONDS;
    return parsed;
};

export const usePlaybackShortcuts = (
    player: PlaybackShortcutApi,
    onToggleFullscreen: () => Promise<void>,
    onToggleInfo: () => void,
    onSeekByArrow?: (deltaSeconds: number) => void,
    onVolumeByArrow?: (volume: number) => void,
    onZoom?: (scale: number) => void,
    onSpeedStep?: (speed: number) => void,
    onSurroundShortcut?: (label: string, value: string) => void,
) => {
    let clickTimer: number | null = null;
    const seekStepSeconds = ref(DEFAULT_SEEK_STEP_SECONDS);
    const pressedKeys = new Set<string>();

    const handleGlobalKeydown = (e: KeyboardEvent) => pressedKeys.add(e.code);
    const handleGlobalKeyup = (e: KeyboardEvent) => pressedKeys.delete(e.code);

    const { surroundState, setEnabled: setSurroundEnabled, setPreset: setSurroundPreset, setParam: setSurroundParam } = useSurroundSound();
    // Current zoom scale (1.0 = no zoom). Stored locally so repeated Cmd+/-
    // accumulates correctly without round-trip latency to mpv.
    let currentZoomScale = 1.0;
    // Current speed scale (1.0 = normal). Same rationale.
    let currentSpeedScale = 1.0;

    const refreshSeekStepFromSettings = async () => {
        const stored = await loadUiState<{
            settings?: {
                groups?: StoredSettingGroup[];
            };
        }>();
        seekStepSeconds.value = parseSeekStepSeconds(stored?.settings?.groups);
    };

    const onSettingsUpdated = (event: Event) => {
        const customEvent = event as CustomEvent<{ groups?: StoredSettingGroup[] }>;
        seekStepSeconds.value = parseSeekStepSeconds(customEvent.detail?.groups);
    };

    onMounted(() => {
        void refreshSeekStepFromSettings();
        window.addEventListener("keydown", handleGlobalKeydown);
        window.addEventListener("keyup", handleGlobalKeyup);
        window.addEventListener(
            SETTINGS_UPDATED_EVENT, onSettingsUpdated);
    });

    onUnmounted(() => {
        window.removeEventListener("keydown", handleGlobalKeydown);
        window.removeEventListener("keyup", handleGlobalKeyup);
        window.removeEventListener(
            SETTINGS_UPDATED_EVENT, onSettingsUpdated);
    });

    const isNonUiTarget = (target: HTMLElement | null) => {
        if (!target) return false;
        if (target.closest(".player-controls")) return false;
        if (target.closest(".top-bar")) return false;
        if (target.closest(".main-panels")) return false;
        return true;
    };

    const onDoubleClick = async (event: MouseEvent) => {
        if (!isNonUiTarget(event.target as HTMLElement | null)) return;
        if (clickTimer !== null) {
            window.clearTimeout(clickTimer);
            clickTimer = null;
        }
        await onToggleFullscreen();
    };

    const onKeydown = async (event: KeyboardEvent) => {
        if (event.code === "Escape") {
            if (!player.state.window.isFullscreen) {
                return;
            }
            event.preventDefault();
            await onToggleFullscreen();
            return;
        }

        const target = event.target as HTMLElement | null;
        const tag = target?.tagName?.toLowerCase();
        if (
            tag === "input" ||
            tag === "textarea" ||
            (target && target.isContentEditable)
        ) {
            return;
        }

        if (
            event.code !== "Space" &&
            event.code !== "ArrowLeft" &&
            event.code !== "ArrowRight" &&
            event.code !== "ArrowUp" &&
            event.code !== "ArrowDown" &&
            event.code !== "KeyI" &&
            // Cmd+= / Cmd+- (zoom) and Cmd+Shift+= / Cmd+Shift+- (speed)
            !(event.metaKey && (event.code === "Equal" || event.code === "Minus")) &&
            // Cmd+Shift+E (toggle surround)
            !(event.metaKey && event.shiftKey && event.code === "KeyE") &&
            // Option+Shift+1/2/3 (surround presets)
            !(event.altKey && event.shiftKey && (event.code === "Digit1" || event.code === "Digit2" || event.code === "Digit3")) &&
            // Option+Shift+... + / - (surround params)
            !(event.altKey && event.shiftKey && (event.code === "Equal" || event.code === "Minus"))
        ) {
            return;
        }

        // --- Surround Shortcuts ---
        if (event.metaKey && event.shiftKey && event.code === "KeyE") {
            event.preventDefault();
            const nextState = !surroundState.value.enabled;
            setSurroundEnabled(nextState);
            onSurroundShortcut?.("3D Audio", nextState ? "Enabled" : "Disabled");
            return;
        }

        if (event.altKey && event.shiftKey && (event.code === "Digit1" || event.code === "Digit2" || event.code === "Digit3")) {
            event.preventDefault();
            let preset: SurroundPreset = "movies";
            let label = "Movies";
            if (event.code === "Digit2") { preset = "music"; label = "Music"; }
            if (event.code === "Digit3") { preset = "gaming"; label = "Gaming"; }
            setSurroundPreset(preset);
            onSurroundShortcut?.("3D Audio Preset", label);
            return;
        }

        if (event.altKey && event.shiftKey && (event.code === "Equal" || event.code === "Minus")) {
            event.preventDefault();
            const delta = event.code === "Equal" ? 1 : -1;
            let param: keyof typeof surroundState.value | null = null;
            let label = "";

            if (pressedKeys.has("KeyS")) { param = "surroundDepth"; label = "Surround Depth"; }
            else if (pressedKeys.has("KeyA")) { param = "ambience"; label = "Ambience"; }
            else if (pressedKeys.has("KeyC")) { param = "clarity"; label = "Clarity"; }
            else if (pressedKeys.has("KeyB")) { param = "bassBoost"; label = "Bass Boost"; }
            else if (pressedKeys.has("KeyD")) { param = "dynamicBoost"; label = "Dynamic Boost"; }

            if (param && typeof surroundState.value[param] === "number") {
                const currentVal = surroundState.value[param] as number;
                const nextVal = Math.min(100, Math.max(0, currentVal + delta));
                setSurroundParam(param, nextVal);
                onSurroundShortcut?.(label, String(nextVal));
            }
            return;
        }
        // --------------------------
        // Handle Cmd+= (zoom in) and Cmd+- (zoom out)
        if (event.metaKey && !event.shiftKey && (event.code === "Equal" || event.code === "Minus")) {
            if (!player.state.media.isFileLoaded) return;
            event.preventDefault();
            const delta = event.code === "Equal" ? ZOOM_STEP : -ZOOM_STEP;
            const nextScale = Math.round(
                Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, currentZoomScale + delta)) * 100,
            ) / 100;
            currentZoomScale = nextScale;
            onZoom?.(nextScale);
            return;
        }
        // Handle Cmd+Shift+= (speed up) and Cmd+Shift+- (speed down)
        if (event.metaKey && event.shiftKey && (event.code === "Equal" || event.code === "Minus")) {
            if (!player.state.media.isFileLoaded) return;
            event.preventDefault();
            const delta = event.code === "Equal" ? SPEED_STEP : -SPEED_STEP;
            const nextSpeed = Math.round(
                Math.min(SPEED_MAX, Math.max(SPEED_MIN, currentSpeedScale + delta)) * 100,
            ) / 100;
            currentSpeedScale = nextSpeed;
            onSpeedStep?.(nextSpeed);
            return;
        }
        if (event.code === "KeyI") {
            if (!player.state.media.isFileLoaded) return;
            event.preventDefault();
            onToggleInfo();
            return;
        }
        if (event.code === "Space") {
            event.preventDefault();
            await player.togglePlayPause();
            return;
        }
        if (event.code === "ArrowUp" || event.code === "ArrowDown") {
            if (!player.state.media.isFileLoaded) return;
            event.preventDefault();
            const delta = event.code === "ArrowUp" ? VOLUME_STEP : -VOLUME_STEP;
            await player.setVolume(player.state.playback.volume + delta);
            onVolumeByArrow?.(player.state.playback.volume);
            return;
        }
        if (
            !player.state.media.isFileLoaded ||
            player.state.media.isLivePlayback ||
            player.state.playback.duration <= 0
        ) {
            return;
        }
        event.preventDefault();
        const delta =
            event.code === "ArrowLeft"
                ? -seekStepSeconds.value
                : seekStepSeconds.value;
        onSeekByArrow?.(delta);
        await player.seekRelative(delta);
    };

    return {
        onDoubleClick,
        onKeydown,
    };
};
