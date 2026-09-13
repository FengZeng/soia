import { confirm } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { open } from "@tauri-apps/plugin-dialog";
import { ref } from "vue";
import {
    defaultSettingGroups,
    IMAGE_DISPLAY_DURATION_SETTING_LABEL,
    LOG_LEVEL_SETTING_LABEL,
    LOG_PATH_SETTING_LABEL,
    NETWORK_PARALLEL_DOWNLOAD_SETTING_LABEL,
    PROXY_ADDRESS_SETTING_LABEL,
    PROXY_MODE_SETTING_LABEL,
    WALLPAPER_MODE_SETTING_LABEL,
    YTDL_COOKIES_FROM_BROWSER_SETTING_LABEL,
    YTDL_MAX_RESOLUTION_SETTING_LABEL,
    YTDL_PATH_SETTING_LABEL,
    type SettingGroup,
    type SettingItem,
} from "../../mock/settings";
import { applyThemeFromSettingGroups } from "../../constants/theme";
import { applyWindowDecorationsFromSettingGroups } from "../../constants/windowDecorations";
import {
    applyImageDisplayDuration,
    applyLoggingSettings,
    applyProxySettings,
    applyStreamProxySettings,
    applyYtdlSettings,
    openLogDirectory,
} from "../useUiStateStore";
import {
    getSettingsLocale,
    translateSettingsText,
} from "../../i18n";

export type StoredSettingItem = { label: string; value: string };
export type StoredSettingGroup = { title: string; items: StoredSettingItem[] };

const DEV_ONLY_SETTING_LABELS = new Set<string>();

const normalizeLogLevel = (value: string): string | null => {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const raw = trimmed.includes("=")
        ? trimmed.slice(trimmed.indexOf("=") + 1)
        : trimmed;
    const normalized = raw.toLowerCase();
    if (normalized === "error" || normalized === "err") return "Error";
    if (normalized === "warn" || normalized === "warning") return "Warn";
    if (normalized === "info" || normalized === "v" || normalized === "verbose") {
        return "Info";
    }
    if (normalized === "debug") return "Debug";
    if (normalized === "trace") return "Trace";
    return null;
};

const cloneSettingGroups = () =>
    defaultSettingGroups.map((group) => ({
        title: group.title,
        items: group.items
            .filter(
                (item) =>
                    import.meta.env.DEV || !DEV_ONLY_SETTING_LABELS.has(item.label),
            )
            .map((item) => {
                if (item.type === "select") {
                    return {
                        ...item,
                        options: [...item.options],
                    };
                }
                return { ...item };
            }),
    }));

const mergeSettingGroups = (stored?: StoredSettingGroup[]): SettingGroup[] => {
    const base = cloneSettingGroups();
    if (!stored?.length) return base;

    base.forEach((group) => {
        const storedGroup = stored.find((candidate) => candidate.title === group.title);
        if (!storedGroup) return;
        group.items.forEach((item) => {
            const storedItem = storedGroup.items.find(
                (candidate) => candidate.label === item.label,
            );
            if (!storedItem?.value) return;
            if (item.type === "select") {
                if (item.options.includes(storedItem.value)) {
                    item.value = storedItem.value;
                } else if (item.label === LOG_LEVEL_SETTING_LABEL) {
                    const normalized = normalizeLogLevel(storedItem.value);
                    if (normalized && item.options.includes(normalized)) {
                        item.value = normalized;
                    }
                }
                return;
            }
            if (item.type === "toggle") {
                if (
                    storedItem.value === item.onValue ||
                    storedItem.value === item.offValue
                ) {
                    item.value = storedItem.value;
                }
                return;
            }
            if (item.type === "slider") {
                const parsed = Number.parseFloat(storedItem.value);
                if (!Number.isNaN(parsed)) {
                    item.value = String(parsed);
                }
                return;
            }
            item.value = storedItem.value;
        });
    });

    return base;
};

const findLogPathItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "path" &&
                candidate.label === LOG_PATH_SETTING_LABEL,
        );
    return item?.type === "path" ? item : undefined;
};

const findLogLevelItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "select" &&
                candidate.label === LOG_LEVEL_SETTING_LABEL,
        );
    return item?.type === "select" ? item : undefined;
};

const findYtdlPathItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "path" &&
                candidate.label === YTDL_PATH_SETTING_LABEL,
        );
    return item?.type === "path" ? item : undefined;
};

const findYtdlCookiesFromBrowserItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "select" &&
                candidate.label === YTDL_COOKIES_FROM_BROWSER_SETTING_LABEL,
        );
    return item?.type === "select" ? item : undefined;
};

const findYtdlMaxResolutionItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "select" &&
                candidate.label === YTDL_MAX_RESOLUTION_SETTING_LABEL,
        );
    return item?.type === "select" ? item : undefined;
};

const parseMaxHeight = (value: string | undefined): number | undefined => {
    if (!value) return undefined;
    const match = value.match(/^(\d+)p$/);
    if (!match) return undefined;
    const height = Number.parseInt(match[1], 10);
    return Number.isFinite(height) && height > 0 ? height : undefined;
};

const findProxyModeItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "select" &&
                candidate.label === PROXY_MODE_SETTING_LABEL,
        );
    return item?.type === "select" ? item : undefined;
};

const findProxyAddressItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "text" &&
                candidate.label === PROXY_ADDRESS_SETTING_LABEL,
        );
    return item?.type === "text" ? item : undefined;
};

const markProxyAddressInvalid = (
    item: ReturnType<typeof findProxyAddressItem>,
) => {
    if (!item) return;
    item.validationMessage = "Enter a valid proxy server, for example 127.0.0.1:7890.";
};

const clearProxyAddressValidation = (
    item: ReturnType<typeof findProxyAddressItem>,
) => {
    if (!item) return;
    item.validationMessage = undefined;
};

const markYtdlPathUnavailable = (item: ReturnType<typeof findYtdlPathItem>) => {
    if (!item) return;
    item.validationMessage =
        "The selected yt-dlp file does not exist or is unavailable.";
};

const clearYtdlPathValidation = (item: ReturnType<typeof findYtdlPathItem>) => {
    if (!item) return;
    item.validationMessage = undefined;
};

const findImageDisplayDurationItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "slider" &&
                candidate.label === IMAGE_DISPLAY_DURATION_SETTING_LABEL,
        );
    return item?.type === "slider" ? item : undefined;
};

const findParallelDownloadItem = (groups: SettingGroup[]) => {
    const item = groups
        .flatMap((group) => group.items)
        .find(
            (candidate) =>
                candidate.type === "toggle" &&
                candidate.label === NETWORK_PARALLEL_DOWNLOAD_SETTING_LABEL,
        );
    return item?.type === "toggle" ? item : undefined;
};

const toPersistedGroups = (groups: SettingGroup[]): StoredSettingGroup[] =>
    groups.map((group) => ({
        title: group.title,
        items: group.items.map((item) => ({
            label: item.label,
            value: item.value,
        })),
    }));

const applyVisualSettings = (groups: SettingGroup[]) => {
    const persistedGroups = toPersistedGroups(groups);
    applyThemeFromSettingGroups(persistedGroups);
    void applyWindowDecorationsFromSettingGroups(persistedGroups);
};

export const useGeneralSettingsSection = (isWindowsPlatform: boolean) => {
    const settingGroups = ref<SettingGroup[]>(cloneSettingGroups());
    const LOGGING_APPLY_DELAY_MS = 250;
    const YTDL_APPLY_DELAY_MS = 250;
    const PROXY_APPLY_DELAY_MS = 250;
    const IMAGE_DURATION_APPLY_DELAY_MS = 250;
    const STREAM_PROXY_APPLY_DELAY_MS = 250;
    let loggingApplyTimer: number | null = null;
    let ytdlApplyTimer: number | null = null;
    let proxyApplyTimer: number | null = null;
    let imageDurationApplyTimer: number | null = null;
    let streamProxyApplyTimer: number | null = null;
    let lastAppliedLoggingRequestKey: string | null = null;
    let lastAppliedYtdlRequestKey: string | null = null;
    let lastAppliedProxyRequestKey: string | null = null;
    let lastAppliedImageDurationRequestKey: string | null = null;
    let lastAppliedStreamProxyRequestKey: string | null = null;
    let lastWallpaperModeValue: string | null = null;
    let isWallpaperRestartPromptOpen = false;

    const isFixedLogPathItem = (item: SettingItem): boolean =>
        item.type === "path" && item.label === LOG_PATH_SETTING_LABEL;

    const buildLoggingRequestKey = (
        logLevelItem: ReturnType<typeof findLogLevelItem>,
    ): string =>
        JSON.stringify({
            logLevel: logLevelItem?.value.trim() ?? "",
        });

    const applyLoggingOptions = async (groups: SettingGroup[]) => {
        const logPathItem = findLogPathItem(groups);
        const logLevelItem = findLogLevelItem(groups);
        const inputLogLevel = logLevelItem?.value.trim();
        const requestedLogLevel = inputLogLevel ? inputLogLevel : undefined;
        const requestKey = buildLoggingRequestKey(logLevelItem);

        if (requestKey === lastAppliedLoggingRequestKey) {
            return;
        }

        const applied = await applyLoggingSettings(requestedLogLevel);
        if (!applied) return;

        if (logPathItem && applied.logPath && logPathItem.value !== applied.logPath) {
            logPathItem.value = applied.logPath;
        }
        if (
            logLevelItem &&
            applied.logLevel &&
            logLevelItem.value !== applied.logLevel &&
            logLevelItem.options.includes(applied.logLevel)
        ) {
            logLevelItem.value = applied.logLevel;
        }

        lastAppliedLoggingRequestKey = buildLoggingRequestKey(logLevelItem);
    };

    const buildYtdlRequestKey = (
        ytdlPathItem: ReturnType<typeof findYtdlPathItem>,
        ytdlCookiesItem: ReturnType<typeof findYtdlCookiesFromBrowserItem>,
        ytdlMaxResolutionItem: ReturnType<typeof findYtdlMaxResolutionItem>,
    ): string =>
        JSON.stringify({
            ytdlPath: ytdlPathItem ? ytdlPathItem.value.trim() : null,
            ytdlCookiesFromBrowser: ytdlCookiesItem ? ytdlCookiesItem.value : null,
            ytdlMaxResolution: ytdlMaxResolutionItem ? ytdlMaxResolutionItem.value : null,
        });

    const applyYtdlOptions = async (groups: SettingGroup[]) => {
        const ytdlPathItem = findYtdlPathItem(groups);
        const ytdlCookiesItem = findYtdlCookiesFromBrowserItem(groups);
        const ytdlMaxResolutionItem = findYtdlMaxResolutionItem(groups);
        const requestKey = buildYtdlRequestKey(ytdlPathItem, ytdlCookiesItem, ytdlMaxResolutionItem);

        if (requestKey === lastAppliedYtdlRequestKey) {
            return;
        }

        const inputYtdlPath = ytdlPathItem?.value.trim();
        const requestedYtdlPath = inputYtdlPath ? inputYtdlPath : undefined;
        const cookiesValue = ytdlCookiesItem?.value;
        const requestedCookiesFromBrowser =
            cookiesValue && cookiesValue !== "Off" ? cookiesValue : undefined;
        const requestedMaxHeight = parseMaxHeight(ytdlMaxResolutionItem?.value);
        const applied = await applyYtdlSettings(
            requestedYtdlPath,
            requestedCookiesFromBrowser,
            requestedMaxHeight,
        );
        if (!applied) {
            if (requestedYtdlPath) {
                markYtdlPathUnavailable(ytdlPathItem);
                lastAppliedYtdlRequestKey = buildYtdlRequestKey(ytdlPathItem, ytdlCookiesItem, ytdlMaxResolutionItem);
            }
            return;
        }

        if (ytdlPathItem) {
            if (requestedYtdlPath && !applied.ytdlPath) {
                markYtdlPathUnavailable(ytdlPathItem);
                lastAppliedYtdlRequestKey = buildYtdlRequestKey(ytdlPathItem, ytdlCookiesItem, ytdlMaxResolutionItem);
                return;
            }

            clearYtdlPathValidation(ytdlPathItem);
            const appliedYtdlPath = applied.ytdlPath ?? "";
            if (ytdlPathItem.value !== appliedYtdlPath) {
                ytdlPathItem.value = appliedYtdlPath;
            }
        }

        lastAppliedYtdlRequestKey = buildYtdlRequestKey(ytdlPathItem, ytdlCookiesItem, ytdlMaxResolutionItem);
    };

    const scheduleApplyLoggingOptions = () => {
        if (loggingApplyTimer) {
            window.clearTimeout(loggingApplyTimer);
        }
        loggingApplyTimer = window.setTimeout(() => {
            void applyLoggingOptions(settingGroups.value);
            loggingApplyTimer = null;
        }, LOGGING_APPLY_DELAY_MS);
    };

    const scheduleApplyYtdlOptions = () => {
        if (ytdlApplyTimer) {
            window.clearTimeout(ytdlApplyTimer);
        }
        ytdlApplyTimer = window.setTimeout(() => {
            void applyYtdlOptions(settingGroups.value);
            ytdlApplyTimer = null;
        }, YTDL_APPLY_DELAY_MS);
    };

    const buildProxyRequestKey = (
        proxyModeItem: ReturnType<typeof findProxyModeItem>,
        proxyAddressItem: ReturnType<typeof findProxyAddressItem>,
    ): string =>
        JSON.stringify({
            proxyMode: proxyModeItem?.value.trim() ?? "",
            proxyAddress: proxyAddressItem?.value.trim() ?? "",
        });

    const applyProxyOptions = async (groups: SettingGroup[]) => {
        const proxyModeItem = findProxyModeItem(groups);
        const proxyAddressItem = findProxyAddressItem(groups);
        const requestKey = buildProxyRequestKey(proxyModeItem, proxyAddressItem);
        if (requestKey === lastAppliedProxyRequestKey) {
            return;
        }

        const requestedMode = proxyModeItem?.value.trim() || undefined;
        const requestedAddress = proxyAddressItem?.value.trim() || undefined;
        const applied = await applyProxySettings(requestedMode, requestedAddress);
        if (!applied) {
            if ((requestedMode ?? "Off") !== "Off") {
                markProxyAddressInvalid(proxyAddressItem);
                lastAppliedProxyRequestKey = buildProxyRequestKey(
                    proxyModeItem,
                    proxyAddressItem,
                );
            }
            return;
        }

        if (proxyModeItem && proxyModeItem.value !== applied.proxyMode) {
            proxyModeItem.value = applied.proxyMode;
        }
        if (proxyAddressItem) {
            const appliedAddress = applied.proxyAddress ?? "";
            if (proxyAddressItem.value !== appliedAddress) {
                proxyAddressItem.value = appliedAddress;
            }
            clearProxyAddressValidation(proxyAddressItem);
        }

        lastAppliedProxyRequestKey = buildProxyRequestKey(
            findProxyModeItem(groups),
            findProxyAddressItem(groups),
        );
    };

    const scheduleApplyProxyOptions = () => {
        if (proxyApplyTimer) {
            window.clearTimeout(proxyApplyTimer);
        }
        proxyApplyTimer = window.setTimeout(() => {
            void applyProxyOptions(settingGroups.value);
            proxyApplyTimer = null;
        }, PROXY_APPLY_DELAY_MS);
    };

    const buildImageDurationRequestKey = (
        durationItem: ReturnType<typeof findImageDisplayDurationItem>,
    ): string =>
        JSON.stringify({
            imageDisplayDuration: durationItem?.value.trim() ?? "",
        });

    const applyImageDurationOptions = async (groups: SettingGroup[]) => {
        const durationItem = findImageDisplayDurationItem(groups);
        const requestKey = buildImageDurationRequestKey(durationItem);
        if (requestKey === lastAppliedImageDurationRequestKey) {
            return;
        }

        const parsed = Number.parseFloat(durationItem?.value ?? "");
        const durationSeconds =
            Number.isFinite(parsed) && parsed > 0 ? Math.min(60, Math.max(1, parsed)) : 5;

        const applied = await applyImageDisplayDuration(durationSeconds);
        if (!applied) return;

        if (durationItem) {
            const normalized = String(Math.round(durationSeconds));
            if (durationItem.value !== normalized) {
                durationItem.value = normalized;
            }
        }
        lastAppliedImageDurationRequestKey = buildImageDurationRequestKey(
            findImageDisplayDurationItem(groups),
        );
    };

    const scheduleApplyImageDurationOptions = () => {
        if (imageDurationApplyTimer) {
            window.clearTimeout(imageDurationApplyTimer);
        }
        imageDurationApplyTimer = window.setTimeout(() => {
            void applyImageDurationOptions(settingGroups.value);
            imageDurationApplyTimer = null;
        }, IMAGE_DURATION_APPLY_DELAY_MS);
    };

    const buildStreamProxyRequestKey = (
        parallelDownloadItem: ReturnType<typeof findParallelDownloadItem>,
    ): string =>
        JSON.stringify({
            parallelDownload: parallelDownloadItem?.value.trim() ?? "",
        });

    const applyStreamProxyOptions = async (groups: SettingGroup[]) => {
        const parallelDownloadItem = findParallelDownloadItem(groups);
        const requestKey = buildStreamProxyRequestKey(parallelDownloadItem);
        if (requestKey === lastAppliedStreamProxyRequestKey) {
            return;
        }

        const enabled = parallelDownloadItem?.value !== "Off";
        const applied = await applyStreamProxySettings(enabled);
        if (!applied) return;

        if (parallelDownloadItem) {
            const normalized = applied.parallelDownloadEnabled ? "On" : "Off";
            if (parallelDownloadItem.value !== normalized) {
                parallelDownloadItem.value = normalized;
            }
        }

        lastAppliedStreamProxyRequestKey = buildStreamProxyRequestKey(
            findParallelDownloadItem(groups),
        );
    };

    const scheduleApplyStreamProxyOptions = () => {
        if (streamProxyApplyTimer) {
            window.clearTimeout(streamProxyApplyTimer);
        }
        streamProxyApplyTimer = window.setTimeout(() => {
            void applyStreamProxyOptions(settingGroups.value);
            streamProxyApplyTimer = null;
        }, STREAM_PROXY_APPLY_DELAY_MS);
    };

    const initializeStorageBackedOptions = async (groups: SettingGroup[]) => {
        await Promise.all([
            applyLoggingOptions(groups),
            applyYtdlOptions(groups),
            applyProxyOptions(groups),
            applyImageDurationOptions(groups),
            applyStreamProxyOptions(groups),
        ]);
    };

    const findWallpaperModeValue = (groups: SettingGroup[]): string | null => {
        const item = groups
            .flatMap((group) => group.items)
            .find((candidate) => candidate.label === WALLPAPER_MODE_SETTING_LABEL);
        if (!item) return null;
        const value = item.value.trim();
        return value ? value : null;
    };

    const maybePromptWallpaperModeRestart = async () => {
        if (!isWindowsPlatform || isWallpaperRestartPromptOpen) return;

        const currentValue = findWallpaperModeValue(settingGroups.value);
        if (currentValue === lastWallpaperModeValue) return;

        lastWallpaperModeValue = currentValue;
        isWallpaperRestartPromptOpen = true;
        try {
            const shouldRelaunch = await confirm(
                translateSettingsText(
                    getSettingsLocale(),
                    "Wallpaper Mode change will take effect after restart. Restart now?",
                ),
                {
                    title: translateSettingsText(getSettingsLocale(), "Restart Required"),
                    kind: "info",
                    okLabel: translateSettingsText(getSettingsLocale(), "Restart now"),
                    cancelLabel: translateSettingsText(getSettingsLocale(), "Later"),
                },
            );
            if (shouldRelaunch) {
                await relaunch();
            }
        } catch {
            // Ignore dialog/relaunch errors and keep current runtime session.
        } finally {
            isWallpaperRestartPromptOpen = false;
        }
    };

    const browseForPath = async (item: SettingItem) => {
        if (item.type !== "path") return;
        if (isFixedLogPathItem(item)) {
            try {
                await openLogDirectory();
            } catch {
                // Intentionally ignore open-directory failures in the panel flow.
            }
            return;
        }
        const selected = await open({
            multiple: false,
            directory: false,
            title: translateSettingsText(
                getSettingsLocale(),
                item.browseTitle ?? "Select file",
            ),
        });
        if (selected) {
            item.value = selected as string;
            if (item.label === YTDL_PATH_SETTING_LABEL) {
                item.validationMessage = undefined;
            }
        }
    };

    const applySectionSideEffects = () => {
        applyVisualSettings(settingGroups.value);
        scheduleApplyLoggingOptions();
        scheduleApplyYtdlOptions();
        scheduleApplyProxyOptions();
        scheduleApplyImageDurationOptions();
        scheduleApplyStreamProxyOptions();
        void maybePromptWallpaperModeRestart();
    };

    const resetGeneralSettings = () => {
        settingGroups.value = cloneSettingGroups();
        void initializeStorageBackedOptions(settingGroups.value);
        applyVisualSettings(settingGroups.value);
    };

    const loadGeneralSettings = async (storedGroups?: StoredSettingGroup[]) => {
        const groups = mergeSettingGroups(storedGroups);
        await initializeStorageBackedOptions(groups);
        settingGroups.value = groups;
        lastWallpaperModeValue = findWallpaperModeValue(groups);
        applyVisualSettings(settingGroups.value);
    };

    const dispose = () => {
        if (loggingApplyTimer) {
            window.clearTimeout(loggingApplyTimer);
            loggingApplyTimer = null;
        }
        if (ytdlApplyTimer) {
            window.clearTimeout(ytdlApplyTimer);
            ytdlApplyTimer = null;
        }
        if (proxyApplyTimer) {
            window.clearTimeout(proxyApplyTimer);
            proxyApplyTimer = null;
        }
        if (imageDurationApplyTimer) {
            window.clearTimeout(imageDurationApplyTimer);
            imageDurationApplyTimer = null;
        }
        if (streamProxyApplyTimer) {
            window.clearTimeout(streamProxyApplyTimer);
            streamProxyApplyTimer = null;
        }
    };

    return {
        settingGroups,
        isFixedLogPathItem,
        browseForPath,
        applySectionSideEffects,
        resetGeneralSettings,
        loadGeneralSettings,
        dispose,
        toPersistedGroups: () => toPersistedGroups(settingGroups.value),
    };
};
