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

type PersistedPlaybackAdjustmentsState = {
    globalColorAdjustmentsEnabled?: boolean;
    globalColorAdjustments?: Partial<ColorAdjustmentsState>;
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
    const persistedStateSaver = createDebouncedUiStateSaver(350);

    const activeColorAdjustments = computed(() =>
        globalColorAdjustmentsEnabled.value
            ? globalColorAdjustments.value
            : localColorAdjustments.value,
    );

    const brightness = computed(() => activeColorAdjustments.value.brightness);
    const contrast = computed(() => activeColorAdjustments.value.contrast);
    const saturation = computed(() => activeColorAdjustments.value.saturation);
    const gamma = computed(() => activeColorAdjustments.value.gamma);
    const hue = computed(() => activeColorAdjustments.value.hue);

    const buildPersistedPlaybackAdjustmentsState = () => ({
        playbackAdjustments: {
            globalColorAdjustmentsEnabled: globalColorAdjustmentsEnabled.value,
            globalColorAdjustments: { ...globalColorAdjustments.value },
        } satisfies PersistedPlaybackAdjustmentsState,
    });

    const persistPlaybackAdjustmentsDebounced = () => {
        if (!globalColorAdjustmentsEnabled.value) return;
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

    void (async () => {
        const stored = await loadUiState<{
            playbackAdjustments?: PersistedPlaybackAdjustmentsState;
        }>();
        const persisted = stored?.playbackAdjustments;
        const enabled = persisted?.globalColorAdjustmentsEnabled === true;
        globalColorAdjustments.value = normalizeColorAdjustments(
            persisted?.globalColorAdjustments,
        );
        globalColorAdjustmentsEnabled.value = enabled;
        if (!enabled) return;
        await reapplyGlobalColorAdjustments();
    })();

    return {
        showSettingsMenu,
        audioDelay,
        subDelay,
        secondarySubDelay,
        brightness,
        contrast,
        saturation,
        gamma,
        hue,
        globalColorAdjustmentsEnabled,
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
        reapplyGlobalColorAdjustments,
        applyColorAdjustmentsForMedia,
    };
};
