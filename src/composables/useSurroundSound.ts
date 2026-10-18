import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { loadUiState, saveUiState } from "./useUiStateStore";

export type SurroundPreset = "off" | "movies" | "music" | "gaming" | "custom";

export type SurroundState = {
    enabled: boolean;
    preset: SurroundPreset;
    surroundDepth: number;
    ambience: number;
    clarity: number;
    bassBoost: number;
    dynamicBoost: number;
};

type PresetValues = Omit<SurroundState, "enabled" | "preset">;

type StoredSurroundPayload = Partial<SurroundState> & {
    globalEnabled?: boolean;
};

export const SURROUND_PRESETS: Record<
    Exclude<SurroundPreset, "off" | "custom">,
    PresetValues
> = {
    movies: {
        surroundDepth: 28,
        ambience: 16,
        clarity: 45,
        bassBoost: 35,
        dynamicBoost: 45,
    },
    music: {
        surroundDepth: 18,
        ambience: 10,
        clarity: 58,
        bassBoost: 32,
        dynamicBoost: 20,
    },
    gaming: {
        surroundDepth: 34,
        ambience: 20,
        clarity: 52,
        bassBoost: 40,
        dynamicBoost: 55,
    },
};

const DEFAULT_STATE: SurroundState = {
    enabled: false,
    preset: "off",
    surroundDepth: 28,
    ambience: 16,
    clarity: 45,
    bassBoost: 35,
    dynamicBoost: 30,
};

const STATE_KEY = "surroundSound";
const LOCAL_SURROUND_MAX_ENTRIES = 100;

const buildFilterChain = async (state: SurroundState): Promise<string> => {
    if (!state.enabled) return "";
    const parts: string[] = [];

    // Dynamic loudness leveling
    if (state.dynamicBoost > 0) {
        const gain = Math.round(8 + (state.dynamicBoost / 100) * 16);
        parts.push(`dynaudnorm=f=150:g=${gain}:p=0.95`);
    }

    // Stereo field widening (tuned for natural separation without phase hollowness)
    if (state.surroundDepth > 0) {
        const width = (1.0 + (state.surroundDepth / 100) * 0.65).toFixed(2);
        parts.push(`extrastereo=m=${width}`);
    }

    // Subtle room ambience
    if (state.ambience > 0) {
        const delay = Math.round(25 + (state.ambience / 100) * 45);
        const decay = (0.05 + (state.ambience / 100) * 0.25).toFixed(2);
        parts.push(`aecho=0.85:0.35:${delay}:${decay}`);
    }

    // Low-frequency gain
    if (state.bassBoost > 0) {
        const gain = ((state.bassBoost / 100) * 12).toFixed(1);
        parts.push(`bass=g=${gain}`);
    }

    // High-frequency treble
    if (state.clarity > 0) {
        const gain = ((state.clarity / 100) * 8).toFixed(1);
        parts.push(`treble=g=${gain}`);
    }

    return parts.join(",");
};

// Module-level state shared across playback controls and settings
const globalSurroundEnabled = ref(false);
const globalSurroundState = ref<SurroundState>({ ...DEFAULT_STATE });
const localSurroundState = ref<SurroundState>({ ...DEFAULT_STATE });
const localSurroundByMediaKey = new Map<string, SurroundState>();
const currentLocalMediaKey = ref("");

const activeSurroundState = computed(() =>
    globalSurroundEnabled.value
        ? globalSurroundState.value
        : localSurroundState.value,
);

let isLoaded = false;
let applyTimer: ReturnType<typeof setTimeout> | null = null;

const persistGlobalStateNow = async () => {
    await saveUiState({
        [STATE_KEY]: {
            globalEnabled: globalSurroundEnabled.value,
            ...globalSurroundState.value,
        },
    });
};

const applyFiltersToMpv = async () => {
    const filterString = await buildFilterChain(activeSurroundState.value);
    try {
        await invoke("mpv_run_command", {
            args: ["set", "af", filterString],
        });
    } catch {
        // Silently ignore if no media is currently playing
    }
};

const scheduleApply = () => {
    if (applyTimer !== null) clearTimeout(applyTimer);
    applyTimer = setTimeout(() => {
        applyTimer = null;
        void applyFiltersToMpv();
    }, 80);
};

export const useSurroundSound = () => {
    if (!isLoaded) {
        isLoaded = true;
        void loadUiState<Record<string, StoredSurroundPayload>>().then(
            (stored) => {
                if (stored?.[STATE_KEY]) {
                    const data = stored[STATE_KEY];
                    globalSurroundEnabled.value = Boolean(data.globalEnabled);
                    globalSurroundState.value = {
                        enabled: Boolean(data.enabled),
                        preset: data.preset ?? DEFAULT_STATE.preset,
                        surroundDepth:
                            data.surroundDepth ?? DEFAULT_STATE.surroundDepth,
                        ambience: data.ambience ?? DEFAULT_STATE.ambience,
                        clarity: data.clarity ?? DEFAULT_STATE.clarity,
                        bassBoost: data.bassBoost ?? DEFAULT_STATE.bassBoost,
                        dynamicBoost:
                            data.dynamicBoost ?? DEFAULT_STATE.dynamicBoost,
                    };
                }
                if (
                    globalSurroundEnabled.value &&
                    globalSurroundState.value.enabled
                ) {
                    void applyFiltersToMpv();
                }
            },
        );
    }

    const setGlobalSurroundEnabled = async (enabled: boolean) => {
        globalSurroundEnabled.value = enabled;
        if (enabled) {
            // Copy active local state into global state if enabling global
            globalSurroundState.value = { ...localSurroundState.value };
        }
        await persistGlobalStateNow();
        scheduleApply();
    };

    const setEnabled = (enabled: boolean) => {
        const target = globalSurroundEnabled.value
            ? globalSurroundState
            : localSurroundState;

        target.value = {
            ...target.value,
            enabled,
            preset: enabled
                ? target.value.preset === "off"
                    ? "custom"
                    : target.value.preset
                : "off",
        };

        if (globalSurroundEnabled.value) {
            void persistGlobalStateNow();
        } else if (currentLocalMediaKey.value) {
            localSurroundByMediaKey.set(currentLocalMediaKey.value, {
                ...target.value,
            });
        }
        scheduleApply();
    };

    const setPreset = (preset: SurroundPreset) => {
        const target = globalSurroundEnabled.value
            ? globalSurroundState
            : localSurroundState;

        if (preset === "off") {
            target.value = {
                ...target.value,
                enabled: false,
                preset: "off",
            };
        } else if (preset === "custom") {
            target.value = { ...target.value, preset: "custom" };
        } else {
            target.value = {
                ...target.value,
                ...SURROUND_PRESETS[preset],
                enabled: true,
                preset,
            };
        }

        if (globalSurroundEnabled.value) {
            void persistGlobalStateNow();
        } else if (currentLocalMediaKey.value) {
            localSurroundByMediaKey.set(currentLocalMediaKey.value, {
                ...target.value,
            });
        }
        scheduleApply();
    };

    const setParam = (key: keyof PresetValues, value: number) => {
        const target = globalSurroundEnabled.value
            ? globalSurroundState
            : localSurroundState;

        target.value = {
            ...target.value,
            [key]: value,
            preset: "custom",
            enabled: true,
        };

        if (globalSurroundEnabled.value) {
            void persistGlobalStateNow();
        } else if (currentLocalMediaKey.value) {
            localSurroundByMediaKey.set(currentLocalMediaKey.value, {
                ...target.value,
            });
        }
        scheduleApply();
    };

    const applySurroundForMedia = async (mediaKey: string) => {
        const normalizedKey = mediaKey.trim();
        currentLocalMediaKey.value = normalizedKey;

        if (globalSurroundEnabled.value) {
            if (globalSurroundState.value.enabled) {
                await applyFiltersToMpv();
            }
            return;
        }

        const storedPerMedia = normalizedKey
            ? localSurroundByMediaKey.get(normalizedKey)
            : undefined;
        const perMedia = storedPerMedia ?? DEFAULT_STATE;

        if (normalizedKey && storedPerMedia) {
            localSurroundByMediaKey.delete(normalizedKey);
            localSurroundByMediaKey.set(normalizedKey, { ...storedPerMedia });
        } else if (
            localSurroundByMediaKey.size > LOCAL_SURROUND_MAX_ENTRIES
        ) {
            const oldestKey = localSurroundByMediaKey.keys().next().value;
            if (oldestKey) localSurroundByMediaKey.delete(oldestKey);
        }

        localSurroundState.value = { ...perMedia };
        if (localSurroundState.value.enabled) {
            await applyFiltersToMpv();
        }
    };

    const reapplyFilters = () => {
        if (activeSurroundState.value.enabled) {
            void applyFiltersToMpv();
        }
    };

    return {
        surroundState: activeSurroundState,
        globalSurroundEnabled,
        setGlobalSurroundEnabled,
        setEnabled,
        setPreset,
        setParam,
        applySurroundForMedia,
        reapplyFilters,
    };
};
