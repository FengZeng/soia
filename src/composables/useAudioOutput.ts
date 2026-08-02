import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onMounted, readonly, ref } from "vue";
import {
    defaultAudioOutputStatus,
    defaultAudioSettings,
    type AudioDevice,
    type AudioOutputStatus,
    type AudioSettings,
} from "../types/audio";

const settings = ref<AudioSettings>(defaultAudioSettings());
const devices = ref<AudioDevice[]>([]);
const status = ref<AudioOutputStatus>(defaultAudioOutputStatus());
const error = ref("");
const isReady = ref(false);
let initializePromise: Promise<void> | null = null;
let listenersPromise: Promise<void> | null = null;
let unlistenDevices: UnlistenFn | null = null;
let unlistenStatus: UnlistenFn | null = null;

const ensureListeners = async () => {
    if (listenersPromise) return listenersPromise;
    listenersPromise = Promise.all([
        listen<AudioDevice[]>("soia:audio-devices", (event) => {
            devices.value = event.payload ?? [];
        }).then((unlisten) => {
            unlistenDevices = unlisten;
        }),
        listen<AudioOutputStatus>("soia:audio-output-status", (event) => {
            status.value = event.payload;
        }).then((unlisten) => {
            unlistenStatus = unlisten;
        }),
    ])
        .then(() => undefined)
        .catch((cause) => {
            listenersPromise = null;
            error.value = String(cause);
        });
    return listenersPromise;
};

const refresh = async () => {
    await ensureListeners();
    try {
        const [nextSettings, nextDevices, nextStatus] = await Promise.all([
            invoke<AudioSettings>("get_audio_settings"),
            invoke<AudioDevice[]>("get_audio_devices"),
            invoke<AudioOutputStatus>("get_audio_output_status"),
        ]);
        settings.value = nextSettings;
        devices.value = nextDevices;
        status.value = nextStatus;
        error.value = "";
        isReady.value = true;
    } catch (cause) {
        error.value = String(cause);
    }
};

const initialize = () => {
    if (!initializePromise) {
        initializePromise = refresh().finally(() => {
            initializePromise = null;
        });
    }
    return initializePromise;
};

const applySettings = async (requested: AudioSettings) => {
    try {
        const applied = await invoke<AudioSettings>("apply_audio_settings", {
            settings: requested,
        });
        settings.value = applied;
        error.value = "";
        return applied;
    } catch (cause) {
        error.value = String(cause);
        throw cause;
    }
};

const retryOutput = async () => {
    try {
        await invoke("retry_audio_output");
        error.value = "";
    } catch (cause) {
        error.value = String(cause);
    }
};

export const useAudioOutput = () => {
    onMounted(() => {
        void initialize();
    });

    return {
        settings: readonly(settings),
        devices: readonly(devices),
        status: readonly(status),
        error: readonly(error),
        isReady: readonly(isReady),
        passthroughActive: computed(() => status.value.passthroughActive),
        refresh,
        applySettings,
        retryOutput,
    };
};

// Retain listeners for the application lifetime. The composable is shared by
// the player and settings panel, so component-level teardown would make one
// consumer accidentally disconnect the other.
void unlistenDevices;
void unlistenStatus;
