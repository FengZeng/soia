import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { SubtitleTarget } from "./useSubtitleState";
import {
    createDebouncedUiStateSaver,
    loadUiState,
    saveUiState,
} from "./useUiStateStore";

const SUB_DELAY_MAX_ENTRIES = 200;

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
    subDelayByMedia?: Record<string, number>;
    secondarySubDelayByMedia?: Record<string, number>;
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
    const subDelayByMediaKey = new Map<string, number>();
    const secondarySubDelayByMediaKey = new Map<string, number>();
    const currentLocalMediaKey = ref("");
    const globalColorAdjustments = ref<ColorAdjustmentsState>({
        ...DEFAULT_COLOR_ADJUSTMENTS,
    });
    const globalColorAdjustmentsEnabled = ref(false);
    const persistedStateSaver = createDebouncedUiStateSaver(350);
    const subDelaySaver = createDebouncedUiStateSaver(500);

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

    const serializeDelayMap = (map: Map<string, number>): Record<string, number> => {
        const result: Record<string, number> = {};
        for (const [key, value] of map) {
            if (value !== 0) result[key] = value;
        }
        return result;
    };

    const buildPersistedPlaybackAdjustmentsState = () => ({
        playbackAdjustments: {
            globalColorAdjustmentsEnabled: globalColorAdjustmentsEnabled.value,
            globalColorAdjustments: { ...globalColorAdjustments.value },
            subDelayByMedia: serializeDelayMap(subDelayByMediaKey),
            secondarySubDelayByMedia: serializeDelayMap(secondarySubDelayByMediaKey),
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
        await hydrationReady;
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

    const evictOldestFromMap = (map: Map<string, number>, maxSize: number) => {
        if (map.size <= maxSize) return;
        const oldestKey = map.keys().next().value;
        if (oldestKey) map.delete(oldestKey);
    };

    const persistSubDelaysDebounced = () => {
        subDelaySaver.saveDebounced(buildPersistedPlaybackAdjustmentsState());
    };

    const setSubDelay = async (value: number) => {
        const next = clamp(value, -300, 300);
        subDelay.value = next;
        if (currentLocalMediaKey.value) {
            subDelayByMediaKey.delete(currentLocalMediaKey.value);
            subDelayByMediaKey.set(currentLocalMediaKey.value, next);
            evictOldestFromMap(subDelayByMediaKey, SUB_DELAY_MAX_ENTRIES);
            persistSubDelaysDebounced();
        }
        await invoke("mpv_set_option_string", { name: "sub-delay", value: next });
    };

    const setSecondarySubDelay = async (value: number) => {
        const next = clamp(value, -300, 300);
        secondarySubDelay.value = next;
        if (currentLocalMediaKey.value) {
            secondarySubDelayByMediaKey.delete(currentLocalMediaKey.value);
            secondarySubDelayByMediaKey.set(currentLocalMediaKey.value, next);
            evictOldestFromMap(secondarySubDelayByMediaKey, SUB_DELAY_MAX_ENTRIES);
            persistSubDelaysDebounced();
        }
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

    const subStep = async (delta: number) => {
        await invoke("mpv_run_command", {
            args: ["sub-step", String(delta)],
        });
        try {
            const newDelayStr = await invoke<string>("mpv_get_property_string", { name: "sub-delay" });
            const newDelay = parseFloat(newDelayStr);
            if (!Number.isNaN(newDelay)) {
                subDelay.value = newDelay;
                if (currentLocalMediaKey.value) {
                    subDelayByMediaKey.delete(currentLocalMediaKey.value);
                    subDelayByMediaKey.set(currentLocalMediaKey.value, newDelay);
                    evictOldestFromMap(subDelayByMediaKey, SUB_DELAY_MAX_ENTRIES);
                    persistSubDelaysDebounced();
                }
            }
        } catch (e) {
            console.error("Failed to read updated sub-delay after sub-step", e);
        }
    };

    const resetSubDelay = async (target?: SubtitleTarget) => {
        if (!target || target === "primary") {
            await setSubDelay(0);
        }
        if (target === "secondary") {
            await setSecondarySubDelay(0);
        }
    };

    const applySubDelayForMedia = async (mediaKey: string) => {
        await hydrationReady;
        const normalizedKey = mediaKey.trim();
        const storedPrimary = normalizedKey
            ? subDelayByMediaKey.get(normalizedKey)
            : undefined;
        const storedSecondary = normalizedKey
            ? secondarySubDelayByMediaKey.get(normalizedKey)
            : undefined;
        const primaryDelay = storedPrimary ?? 0;
        const secondaryDelay = storedSecondary ?? 0;
        subDelay.value = primaryDelay;
        secondarySubDelay.value = secondaryDelay;

        const sendDelayToMpv = async () => {
            await invoke("mpv_set_option_string", {
                name: "sub-delay",
                value: primaryDelay,
            });
            await invoke("mpv_set_option_string", {
                name: "secondary-sub-delay",
                value: secondaryDelay,
            });
        };

        await sendDelayToMpv();

        // MPV may reset sub-delay when subtitle tracks finish initializing
        // (which happens after the file-loaded event). Verify and re-apply.
        const verifyAndRetry = async () => {
            try {
                const actualStr = await invoke<string>("mpv_get_property_string", { name: "sub-delay" });
                const actual = parseFloat(actualStr);
                if (!Number.isNaN(actual) && Math.abs(actual - primaryDelay) > 0.001) {
                    await sendDelayToMpv();
                }
            } catch { /* mpv not ready yet, ignore */ }
        };

        setTimeout(verifyAndRetry, 500);
        setTimeout(verifyAndRetry, 1500);
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

    const hydrationReady = (async () => {
        const stored = await loadUiState<{
            playbackAdjustments?: PersistedPlaybackAdjustmentsState;
        }>();
        const persisted = stored?.playbackAdjustments;
        const enabled = persisted?.globalColorAdjustmentsEnabled === true;
        globalColorAdjustments.value = normalizeColorAdjustments(
            persisted?.globalColorAdjustments,
        );
        globalColorAdjustmentsEnabled.value = enabled;

        // Restore per-media subtitle delays from persisted state
        if (persisted?.subDelayByMedia && typeof persisted.subDelayByMedia === "object") {
            for (const [key, value] of Object.entries(persisted.subDelayByMedia)) {
                if (typeof value === "number" && Number.isFinite(value) && key.trim()) {
                    subDelayByMediaKey.set(key, clamp(value, -300, 300));
                }
            }
        }
        if (persisted?.secondarySubDelayByMedia && typeof persisted.secondarySubDelayByMedia === "object") {
            for (const [key, value] of Object.entries(persisted.secondarySubDelayByMedia)) {
                if (typeof value === "number" && Number.isFinite(value) && key.trim()) {
                    secondarySubDelayByMediaKey.set(key, clamp(value, -300, 300));
                }
            }
        }

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
        subStep,
        resetSubDelay,
        applySubDelayForMedia,
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
