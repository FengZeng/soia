/**
 * useSurroundSound — Virtual 3D surround effect via mpv audio filters.
 *
 * Uses a module-level singleton so the same reactive state is shared
 * between App.vue (re-apply on file load) and SettingsPanel.vue (UI).
 *
 * Filter chain (when enabled):
 *   dynaudnorm  → dynamic loudness levelling  (Dynamic Boost)
 *   extrastereo → stereo field widening        (Surround Depth)
 *   aecho       → reverb / room ambience       (Ambience)
 *   bass        → low-frequency gain           (Bass Boost)
 *   treble      → high-frequency gain          (Clarity)
 */

import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { loadUiState, saveUiState } from "./useUiStateStore";

// ─── Types ─────────────────────────────────────────────────────────────────

export type SurroundPreset = "off" | "movies" | "music" | "gaming" | "custom";

export type SurroundState = {
    enabled: boolean;
    preset: SurroundPreset;
    /** 0–100 — stereo field width via extrastereo */
    surroundDepth: number;
    /** 0–100 — reverb / room ambience via aecho */
    ambience: number;
    /** 0–100 — treble presence via treble filter */
    clarity: number;
    /** 0–100 — low-frequency boost via bass filter */
    bassBoost: number;
    /** 0–100 — dynamic loudness levelling via dynaudnorm */
    dynamicBoost: number;
};

// ─── Presets ───────────────────────────────────────────────────────────────

type PresetValues = Omit<SurroundState, "enabled" | "preset">;

export const SURROUND_PRESETS: Record<
    Exclude<SurroundPreset, "off" | "custom">,
    PresetValues
> = {
    movies: {
        surroundDepth: 68,
        ambience: 55,
        clarity: 35,
        bassBoost: 42,
        dynamicBoost: 55,
    },
    music: {
        surroundDepth: 35,
        ambience: 22,
        clarity: 62,
        bassBoost: 48,
        dynamicBoost: 28,
    },
    gaming: {
        surroundDepth: 82,
        ambience: 65,
        clarity: 50,
        bassBoost: 55,
        dynamicBoost: 72,
    },
};

const DEFAULT_STATE: SurroundState = {
    enabled: false,
    preset: "off",
    surroundDepth: 50,
    ambience: 30,
    clarity: 40,
    bassBoost: 30,
    dynamicBoost: 20,
};

const STATE_KEY = "surroundSound";

// ─── Filter builder ────────────────────────────────────────────────────────

const buildFilterChain = async (s: SurroundState): Promise<string> => {
    if (!s.enabled) return "";
    const parts: string[] = [];

    // Dynamic loudness levelling
    if (s.dynamicBoost > 0) {
        const g = Math.round(10 + (s.dynamicBoost / 100) * 20);
        parts.push(`dynaudnorm=f=150:g=${g}:p=0.95`);
    }

    // Stereo field widening (Surround Depth)
    if (s.surroundDepth > 0) {
        const m = ((s.surroundDepth / 100) * 2.5).toFixed(2);
        parts.push(`extrastereo=m=${m}`);
    }

    // Reverb / ambience
    if (s.ambience > 0) {
        const delay = Math.round(30 + (s.ambience / 100) * 100);
        const decay = (0.1 + (s.ambience / 100) * 0.55).toFixed(2);
        parts.push(`aecho=0.8:0.85:${delay}:${decay}`);
    }

    // Bass boost
    if (s.bassBoost > 0) {
        const g = ((s.bassBoost / 100) * 12).toFixed(1);
        parts.push(`bass=g=${g}`);
    }

    // Clarity (treble)
    if (s.clarity > 0) {
        const g = ((s.clarity / 100) * 8).toFixed(1);
        parts.push(`treble=g=${g}`);
    }

    return parts.join(",");
};

// ─── Singleton state ────────────────────────────────────────────────────────
// Module-level so App.vue and SettingsPanel share the same instance.

const _state = ref<SurroundState>({ ...DEFAULT_STATE });
let _loaded = false;
let _applyTimer: ReturnType<typeof setTimeout> | null = null;
const _persistState = () => {
    void saveUiState({ [STATE_KEY]: _state.value });
};

const _applyFilters = async () => {
    const chain = await buildFilterChain(_state.value);
    try {
        await invoke("mpv_run_command", {
            args: ["set", "af", chain],
        });
    } catch {
        // Silently ignore — no media loaded yet is normal.
    }
};

const _scheduleApply = () => {
    if (_applyTimer !== null) clearTimeout(_applyTimer);
    _applyTimer = setTimeout(() => {
        _applyTimer = null;
        void _applyFilters();
    }, 80);
};

// ─── Composable ────────────────────────────────────────────────────────────

export const useSurroundSound = () => {
    // Load persisted state once
    if (!_loaded) {
        _loaded = true;
        void loadUiState<Record<string, SurroundState>>().then((stored) => {
            if (stored?.[STATE_KEY]) {
                _state.value = { ...DEFAULT_STATE, ...stored[STATE_KEY] };
            }
            // Re-apply on startup (e.g. app reopen with surround still enabled)
            if (_state.value.enabled) {
                void _applyFilters();
            }
        });
    }

    const setEnabled = (enabled: boolean) => {
        _state.value = {
            ..._state.value,
            enabled,
            preset: enabled ? (_state.value.preset === "off" ? "custom" : _state.value.preset) : "off",
        };
        _persistState();
        _scheduleApply();
    };

    const setPreset = (preset: SurroundPreset) => {
        if (preset === "off") {
            _state.value = { ..._state.value, enabled: false, preset: "off" };
        } else if (preset === "custom") {
            _state.value = { ..._state.value, preset: "custom" };
        } else {
            _state.value = {
                ..._state.value,
                ...SURROUND_PRESETS[preset],
                enabled: true,
                preset,
            };
        }
        _persistState();
        _scheduleApply();
    };

    const setParam = (
        key: keyof PresetValues,
        value: number,
    ) => {
        _state.value = { ..._state.value, [key]: value, preset: "custom", enabled: true };
        _persistState();
        _scheduleApply();
    };

    /** Call this from App.vue's onFileLoaded to re-apply after mpv resets af. */
    const reapplyFilters = () => {
        if (_state.value.enabled) {
            void _applyFilters();
        }
    };

    return {
        surroundState: _state,
        setEnabled,
        setPreset,
        setParam,
        reapplyFilters,
    };
};
