import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { SubtitleTarget } from "./useSubtitleState";
import {
    createDebouncedUiStateSaver,
    loadUiState,
    saveUiState,
} from "./useUiStateStore";

const clamp = (value: number, min: number, max: number) =>
    Math.min(max, Math.max(min, value));

type ColorAdjustmentKey =
    | "brightness"
    | "contrast"
    | "saturation"
    | "gamma"
    | "hue";

type ColorAdjustmentsState = Record<ColorAdjustmentKey, number>;

export type CropZoomState = {
    zoom: number;
    ratio: string;
};

type PersistedPlaybackAdjustmentsState = {
    globalColorAdjustmentsEnabled?: boolean;
    globalColorAdjustments?: Partial<ColorAdjustmentsState>;
    globalCropZoomEnabled?: boolean;
    globalCropZoom?: Partial<CropZoomState>;
};

const COLOR_ADJUSTMENT_KEYS: ColorAdjustmentKey[] = [
    "brightness",
    "contrast",
    "saturation",
    "gamma",
    "hue",
];

const DEFAULT_COLOR_ADJUSTMENTS: ColorAdjustmentsState = {
    brightness: 0,
    contrast: 0,
    saturation: 0,
    gamma: 0,
    hue: 0,
};
const DEFAULT_CROP_ZOOM: CropZoomState = {
    zoom: 1.0,
    ratio: "Auto",
};
const LOCAL_ADJUSTMENTS_MAX_ENTRIES = 100;

const normalizeColorAdjustmentValue = (value: unknown) => {
    if (typeof value !== "number" || !Number.isFinite(value)) return 0;
    return clamp(Math.round(value), -100, 100);
};

const normalizeColorAdjustments = (
    values?: Partial<ColorAdjustmentsState>,
): ColorAdjustmentsState =>
    COLOR_ADJUSTMENT_KEYS.reduce<ColorAdjustmentsState>(
        (acc, key) => {
            const fallback = DEFAULT_COLOR_ADJUSTMENTS[key];
            acc[key] =
                values && key in values
                    ? normalizeColorAdjustmentValue(values[key])
                    : fallback;
            return acc;
        },
        { ...DEFAULT_COLOR_ADJUSTMENTS },
    );

export const usePlaybackAdjustments = () => {
    const showSettingsMenu = ref(false);
    const showCropMenu = ref(false);
    const audioDelay = ref(0);
    const subDelay = ref(0);
    const secondarySubDelay = ref(0);
    const localColorAdjustments = ref<ColorAdjustmentsState>({
        ...DEFAULT_COLOR_ADJUSTMENTS,
    });
    const localColorAdjustmentsByMediaKey = new Map<string, ColorAdjustmentsState>();
    const currentLocalMediaKey = ref("");
    const globalColorAdjustments = ref<ColorAdjustmentsState>({
        ...DEFAULT_COLOR_ADJUSTMENTS,
    });
    const globalColorAdjustmentsEnabled = ref(false);

    const localCropZoom = ref<CropZoomState>({ ...DEFAULT_CROP_ZOOM });
    const localCropZoomByMediaKey = new Map<string, CropZoomState>();
    const globalCropZoom = ref<CropZoomState>({ ...DEFAULT_CROP_ZOOM });
    const globalCropZoomEnabled = ref(false);

    const persistedStateSaver = createDebouncedUiStateSaver(350);

    const activeColorAdjustments = computed(() =>
        globalColorAdjustmentsEnabled.value
            ? globalColorAdjustments.value
            : localColorAdjustments.value,
    );

    const activeCropZoom = computed(() =>
        globalCropZoomEnabled.value
            ? globalCropZoom.value
            : localCropZoom.value,
    );

    const brightness = computed(() => activeColorAdjustments.value.brightness);
    const contrast = computed(() => activeColorAdjustments.value.contrast);
    const saturation = computed(() => activeColorAdjustments.value.saturation);
    const gamma = computed(() => activeColorAdjustments.value.gamma);
    const hue = computed(() => activeColorAdjustments.value.hue);

    const currentCropZoom = computed(() => activeCropZoom.value.zoom);
    const currentCropRatio = computed(() => activeCropZoom.value.ratio);

    const buildPersistedPlaybackAdjustmentsState = () => ({
        playbackAdjustments: {
            globalColorAdjustmentsEnabled: globalColorAdjustmentsEnabled.value,
            globalColorAdjustments: { ...globalColorAdjustments.value },
            globalCropZoomEnabled: globalCropZoomEnabled.value,
            globalCropZoom: { ...globalCropZoom.value },
        } satisfies PersistedPlaybackAdjustmentsState,
    });

    const persistPlaybackAdjustmentsDebounced = () => {
        if (!globalColorAdjustmentsEnabled.value && !globalCropZoomEnabled.value) return;
        persistedStateSaver.saveDebounced(buildPersistedPlaybackAdjustmentsState());
    };

    const persistPlaybackAdjustmentsNow = async () => {
        await saveUiState(buildPersistedPlaybackAdjustmentsState());
    };

    const applyColorAdjustment = async (
        option: ColorAdjustmentKey,
        next: number,
    ) => {
        if (option === "brightness") {
            await invoke<number>("set_brightness_adjustment", { value: next });
            return;
        }
        await invoke("mpv_set_option_string", {
            name: option,
            value: next,
        });
    };

    const reapplyGlobalColorAdjustments = async () => {
        if (!globalColorAdjustmentsEnabled.value) return;
        await Promise.all(
            COLOR_ADJUSTMENT_KEYS.map((key) =>
                applyColorAdjustment(key, globalColorAdjustments.value[key]),
            ),
        );
    };

    const applyColorAdjustmentsSet = async (values: ColorAdjustmentsState) => {
        await Promise.all(
            COLOR_ADJUSTMENT_KEYS.map((key) =>
                applyColorAdjustment(key, values[key]),
            ),
        );
    };

    const setColorAdjustment = async (key: ColorAdjustmentKey, value: number) => {
        const next = clamp(value, -100, 100);
        activeColorAdjustments.value[key] = next;
        if (!globalColorAdjustmentsEnabled.value && currentLocalMediaKey.value) {
            const mediaKey = currentLocalMediaKey.value;
            localColorAdjustmentsByMediaKey.delete(mediaKey);
            localColorAdjustmentsByMediaKey.set(mediaKey, {
                ...localColorAdjustments.value,
            });
            if (localColorAdjustmentsByMediaKey.size > LOCAL_ADJUSTMENTS_MAX_ENTRIES) {
                const oldestKey = localColorAdjustmentsByMediaKey.keys().next().value;
                if (oldestKey) {
                    localColorAdjustmentsByMediaKey.delete(oldestKey);
                }
            }
        }
        await applyColorAdjustment(key, next);
        if (globalColorAdjustmentsEnabled.value) {
            persistPlaybackAdjustmentsDebounced();
        }
    };

    const applyColorAdjustmentsForMedia = async (mediaKey: string) => {
        const normalizedKey = mediaKey.trim();
        currentLocalMediaKey.value = normalizedKey;
        if (globalColorAdjustmentsEnabled.value) {
            await reapplyGlobalColorAdjustments();
            return;
        }

        const storedPerMedia = normalizedKey
            ? localColorAdjustmentsByMediaKey.get(normalizedKey)
            : undefined;
        const perMedia = storedPerMedia ?? DEFAULT_COLOR_ADJUSTMENTS;
        if (normalizedKey && storedPerMedia) {
            localColorAdjustmentsByMediaKey.delete(normalizedKey);
            localColorAdjustmentsByMediaKey.set(normalizedKey, {
                ...storedPerMedia,
            });
        }
        localColorAdjustments.value = { ...perMedia };
        await applyColorAdjustmentsSet(localColorAdjustments.value);
    };

    const setAudioDelay = async (value: number) => {
        const next = clamp(value, -5, 5);
        audioDelay.value = next;
        await invoke("mpv_set_option_string", {
            name: "audio-delay",
            value: next,
        });
    };

    const setSubDelay = async (value: number) => {
        const next = clamp(value, -10, 10);
        subDelay.value = next;
        await invoke("mpv_set_option_string", { name: "sub-delay", value: next });
    };

    const setSecondarySubDelay = async (value: number) => {
        const next = clamp(value, -10, 10);
        secondarySubDelay.value = next;
        await invoke("mpv_set_option_string", {
            name: "secondary-sub-delay",
            value: next,
        });
    };

    const setSubDelayForTarget = async (payload: {
        target: SubtitleTarget;
        value: number;
    }) => {
        if (payload.target === "secondary") {
            await setSecondarySubDelay(payload.value);
            return;
        }
        await setSubDelay(payload.value);
    };

    const setBrightness = async (value: number) => {
        await setColorAdjustment("brightness", value);
    };

    const setContrast = async (value: number) => {
        await setColorAdjustment("contrast", value);
    };

    const setSaturation = async (value: number) => {
        await setColorAdjustment("saturation", value);
    };

    const setGamma = async (value: number) => {
        await setColorAdjustment("gamma", value);
    };

    const setHue = async (value: number) => {
        await setColorAdjustment("hue", value);
    };

    const setGlobalColorAdjustmentsEnabled = async (enabled: boolean) => {
        if (globalColorAdjustmentsEnabled.value === enabled) return;
        persistedStateSaver.cancel();
        globalColorAdjustmentsEnabled.value = enabled;
        await applyColorAdjustmentsSet(activeColorAdjustments.value);
        await persistPlaybackAdjustmentsNow();
    };

    const applyCropZoomToMpv = async (zoom: number) => {
        try {
            const clamped = clamp(zoom, 0.1, 4.0);
            const mpvZoom = Math.log2(clamped);
            await invoke("mpv_run_command", {
                args: ["set", "video-zoom", String(mpvZoom.toFixed(6))],
            });
            await invoke("mpv_run_command", {
                args: ["set", "video-aspect-override", "no"],
            });
        } catch (e) {
            console.error("Failed to apply crop zoom to mpv:", e);
        }
    };

    const setCropZoom = async (zoom: number, ratio: string) => {
        const next: CropZoomState = {
            zoom: clamp(zoom, 0.1, 4.0),
            ratio: ratio || "Auto",
        };
        activeCropZoom.value.zoom = next.zoom;
        activeCropZoom.value.ratio = next.ratio;

        if (!globalCropZoomEnabled.value && currentLocalMediaKey.value) {
            const mediaKey = currentLocalMediaKey.value;
            localCropZoomByMediaKey.delete(mediaKey);
            localCropZoomByMediaKey.set(mediaKey, { ...localCropZoom.value });
            if (localCropZoomByMediaKey.size > LOCAL_ADJUSTMENTS_MAX_ENTRIES) {
                const oldestKey = localCropZoomByMediaKey.keys().next().value;
                if (oldestKey) {
                    localCropZoomByMediaKey.delete(oldestKey);
                }
            }
        }

        await applyCropZoomToMpv(next.zoom);
        if (globalCropZoomEnabled.value) {
            persistPlaybackAdjustmentsDebounced();
        }
    };

    const reapplyGlobalCropZoom = async () => {
        if (!globalCropZoomEnabled.value) return;
        await applyCropZoomToMpv(globalCropZoom.value.zoom);
    };

    const applyCropZoomForMedia = async (mediaKey: string) => {
        const normalizedKey = mediaKey.trim();
        if (globalCropZoomEnabled.value) {
            await reapplyGlobalCropZoom();
            return;
        }

        const storedPerMedia = normalizedKey
            ? localCropZoomByMediaKey.get(normalizedKey)
            : undefined;
        localCropZoom.value = storedPerMedia
            ? { ...storedPerMedia }
            : { ...DEFAULT_CROP_ZOOM };
        await applyCropZoomToMpv(localCropZoom.value.zoom);
    };

    const setGlobalCropZoomEnabled = async (enabled: boolean) => {
        if (globalCropZoomEnabled.value === enabled) return;
        persistedStateSaver.cancel();
        globalCropZoomEnabled.value = enabled;
        await applyCropZoomToMpv(activeCropZoom.value.zoom);
        await persistPlaybackAdjustmentsNow();
    };

    void (async () => {
        const stored = await loadUiState<{
            playbackAdjustments?: PersistedPlaybackAdjustmentsState;
        }>();
        const persisted = stored?.playbackAdjustments;
        const colorEnabled = persisted?.globalColorAdjustmentsEnabled === true;
        globalColorAdjustments.value = normalizeColorAdjustments(
            persisted?.globalColorAdjustments,
        );
        globalColorAdjustmentsEnabled.value = colorEnabled;

        const cropEnabled = persisted?.globalCropZoomEnabled === true;
        if (persisted?.globalCropZoom) {
            globalCropZoom.value = {
                zoom: persisted.globalCropZoom.zoom ?? 1.0,
                ratio: persisted.globalCropZoom.ratio ?? "Auto",
            };
        }
        globalCropZoomEnabled.value = cropEnabled;

        if (colorEnabled) {
            await reapplyGlobalColorAdjustments();
        }
        if (cropEnabled) {
            await reapplyGlobalCropZoom();
        }
    })();

    return {
        showSettingsMenu,
        showCropMenu,
        audioDelay,
        subDelay,
        secondarySubDelay,
        brightness,
        contrast,
        saturation,
        gamma,
        hue,
        globalColorAdjustmentsEnabled,
        globalCropZoomEnabled,
        currentCropZoom,
        currentCropRatio,
        setAudioDelay,
        setSubDelay,
        setSecondarySubDelay,
        setSubDelayForTarget,
        setBrightness,
        setContrast,
        setSaturation,
        setGamma,
        setHue,
        setGlobalColorAdjustmentsEnabled,
        setGlobalCropZoomEnabled,
        setCropZoom,
        reapplyGlobalColorAdjustments,
        reapplyGlobalCropZoom,
        applyColorAdjustmentsForMedia,
        applyCropZoomForMedia,
    };
};
