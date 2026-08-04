import { invoke } from "@tauri-apps/api/core";

type UiStateObject = Record<string, unknown>;
type LoggingSettingsState = {
    logPath: string | null;
    logLevel: string;
};
type YtdlSettingsState = {
    ytdlPath: string | null;
    ytdlCookiesFromBrowser: string | null;
    ytdlMaxHeight: number | null;
};
type ProxySettingsState = {
    proxyMode: string;
    proxyAddress: string | null;
    proxyUrl: string | null;
};
type StreamProxySettingsState = {
    parallelDownloadEnabled: boolean;
};
type RenderingSettingsState = {
    selectedShaderFiles: string[];
    activeShaderFiles: string[];
};
type OnlineSubtitleCacheClearResult = {
    removedFiles: number;
    removedBytes: number;
};

export type PersistedMediaTracksState = {
    audioTrackByMedia?: Record<string, string>;
    subTrackByMedia?: Record<string, string>;
};

let cachedUiState: UiStateObject | null = null;
let hasCachedUiState = false;
let pendingLoad: Promise<UiStateObject | null> | null = null;

const cloneUiState = <T>(value: T): T => {
    if (value === null || value === undefined) return value;
    return JSON.parse(JSON.stringify(value)) as T;
};

const mergeUiState = (
    existing: UiStateObject | null,
    patch: UiStateObject,
): UiStateObject => ({
    ...(existing ?? {}),
    ...patch,
});

export const loadUiState = async <T>(): Promise<T | null> => {
    if (hasCachedUiState) {
        return cloneUiState(cachedUiState) as T | null;
    }

    if (!pendingLoad) {
        pendingLoad = invoke<UiStateObject>("load_ui_state")
            .then((state) => state ?? null)
            .catch(() => null)
            .finally(() => {
                pendingLoad = null;
            });
    }

    const state = await pendingLoad;
    cachedUiState = state;
    hasCachedUiState = true;
    return cloneUiState(state) as T | null;
};

export const saveUiState = async (state: UiStateObject): Promise<void> => {
    const serializableState = cloneUiState(state);
    await invoke("save_ui_state", { state: serializableState });
    if (hasCachedUiState) {
        cachedUiState = mergeUiState(cachedUiState, serializableState);
    }
};

export const factoryReset = async (): Promise<void> => {
    await invoke("factory_reset");
    cachedUiState = null;
    hasCachedUiState = false;
    pendingLoad = null;
};

export const openLogDirectory = async (): Promise<void> => {
    await invoke("open_log_directory");
};

export const applyLoggingSettings = async (
    logLevel?: string,
): Promise<LoggingSettingsState | null> => {
    try {
        return await invoke<LoggingSettingsState>("apply_logging_settings", {
            logLevel,
        });
    } catch {
        return null;
    }
};

export const applyYtdlSettings = async (
    ytdlPath?: string,
    ytdlCookiesFromBrowser?: string,
    ytdlMaxHeight?: number,
): Promise<YtdlSettingsState | null> => {
    try {
        return await invoke<YtdlSettingsState>("apply_ytdl_settings", {
            ytdlPath,
            ytdlCookiesFromBrowser,
            ytdlMaxHeight,
        });
    } catch {
        return null;
    }
};

export const applyProxySettings = async (
    proxyMode?: string,
    proxyAddress?: string,
): Promise<ProxySettingsState | null> => {
    try {
        return await invoke<ProxySettingsState>("apply_proxy_settings", {
            proxyMode,
            proxyAddress,
        });
    } catch {
        return null;
    }
};

export const applyStreamProxySettings = async (
    parallelDownloadEnabled?: boolean,
): Promise<StreamProxySettingsState | null> => {
    try {
        return await invoke<StreamProxySettingsState>("apply_stream_proxy_settings", {
            parallelDownloadEnabled,
        });
    } catch {
        return null;
    }
};

export const applyImageDisplayDuration = async (
    seconds: number,
): Promise<boolean> => {
    try {
        await invoke("mpv_set_option_string", {
            name: "image-display-duration",
            value: seconds,
        });
        return true;
    } catch {
        return false;
    }
};

export const applyRenderingSettings = async (
    selectedShaderFiles: string[],
    activeShaderFiles: string[],
): Promise<RenderingSettingsState | null> => {
    try {
        return await invoke<RenderingSettingsState>("apply_rendering_settings", {
            selectedShaderFiles,
            activeShaderFiles,
        });
    } catch {
        return null;
    }
};

export const clearOnlineSubtitleCache =
    async (): Promise<OnlineSubtitleCacheClearResult> => {
        return await invoke<OnlineSubtitleCacheClearResult>(
            "clear_online_subtitle_cache",
        );
    };

export const resolveShaderCandidates = async (
    paths: string[],
): Promise<string[]> => {
    try {
        return await invoke<string[]>("resolve_shader_candidates", { paths });
    } catch {
        return [];
    }
};

export const resolveExistingShaderFiles = async (
    paths: string[],
): Promise<string[]> => {
    try {
        return await invoke<string[]>("resolve_existing_shader_files", { paths });
    } catch {
        return [];
    }
};

export const pickPathsNative = async (): Promise<string[] | null> => {
    try {
        const selected = await invoke<string[]>("pick_paths_native");
        return Array.isArray(selected) ? selected : [];
    } catch {
        return null;
    }
};

export const createDebouncedUiStateSaver = (delayMs = 300) => {
    let saveTimer: number | null = null;
    let pendingState: UiStateObject | null = null;

    const cancel = () => {
        if (saveTimer) {
            window.clearTimeout(saveTimer);
            saveTimer = null;
        }
    };

    const saveDebounced = (state: UiStateObject) => {
        pendingState = cloneUiState(state);
        cancel();
        saveTimer = window.setTimeout(() => {
            if (!pendingState) return;
            void saveUiState(pendingState);
            pendingState = null;
            saveTimer = null;
        }, delayMs);
    };

    const flush = (state?: UiStateObject) => {
        if (state) {
            pendingState = cloneUiState(state);
        }
        cancel();
        if (!pendingState) return;
        void saveUiState(pendingState);
        pendingState = null;
    };

    return {
        saveDebounced,
        cancel,
        flush,
    };
};
