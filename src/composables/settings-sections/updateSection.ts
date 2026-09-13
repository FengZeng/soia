import { computed, ref } from "vue";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import {
    getSettingsLocale,
    translateSettingsText,
} from "../../i18n";

type UpdateButtonPhase =
    | "idle"
    | "checking"
    | "downloading"
    | "installing"
    | "failed";

const PROJECT_RELEASES_URL = "https://github.com/FengZeng/soia/releases";

const formatByteSize = (bytes: number): string => {
    if (!Number.isFinite(bytes) || bytes < 0) return "0 B";
    if (bytes < 1024) return `${Math.floor(bytes)} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let value = bytes / 1024;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
        value /= 1024;
        unitIndex += 1;
    }
    const precision = value >= 10 ? 1 : 2;
    return `${value.toFixed(precision)} ${units[unitIndex]}`;
};

const isUpdateBusyPhase = (phase: UpdateButtonPhase): boolean =>
    phase === "checking" || phase === "downloading" || phase === "installing";

let cachedAvailableUpdateVersion: string | null = null;
let cachedAvailableUpdateResource: Update | null = null;
let updateWorkflowPromise: Promise<void> | null = null;

const sharedHasAvailableUpdate = ref(false);
const sharedAvailableUpdateVersion = ref<string | null>(cachedAvailableUpdateVersion);
const sharedUpdateButtonPhase = ref<UpdateButtonPhase>("idle");
const sharedUpdateProgressTotalBytes = ref<number | null>(null);
const sharedUpdateProgressDownloadedBytes = ref(0);
const sharedUpdateDownloadSpeedBytesPerSec = ref<number | null>(null);
const sharedUpdateLastProgressAtMs = ref<number | null>(null);
const sharedUpdateLastProgressBytes = ref(0);

export const useUpdateSection = () => {
    const hasAvailableUpdate = sharedHasAvailableUpdate;
    const availableUpdateVersion = sharedAvailableUpdateVersion;
    const updateButtonPhase = sharedUpdateButtonPhase;
    const updateProgressTotalBytes = sharedUpdateProgressTotalBytes;
    const updateProgressDownloadedBytes = sharedUpdateProgressDownloadedBytes;
    const updateDownloadSpeedBytesPerSec = sharedUpdateDownloadSpeedBytesPerSec;
    const updateLastProgressAtMs = sharedUpdateLastProgressAtMs;
    const updateLastProgressBytes = sharedUpdateLastProgressBytes;

    let unlistenUpdateAvailable: UnlistenFn | null = null;

    const closeUpdateResource = async (update: Update | null) => {
        if (!update) return;
        try {
            await update.close();
        } catch {
            // Ignore cleanup failures from updater resource handles.
        }
    };

    const setAvailableUpdateResource = async (update: Update | null) => {
        if (cachedAvailableUpdateResource && cachedAvailableUpdateResource !== update) {
            await closeUpdateResource(cachedAvailableUpdateResource);
        }
        cachedAvailableUpdateResource = update;
        hasAvailableUpdate.value = Boolean(update);
        availableUpdateVersion.value = update?.version?.trim() || null;
        cachedAvailableUpdateVersion = availableUpdateVersion.value;
    };

    const consumeAvailableUpdateResource = () => {
        const update = cachedAvailableUpdateResource;
        cachedAvailableUpdateResource = null;
        return update;
    };

    const loadUpdateAvailability = async () => {
        try {
            hasAvailableUpdate.value = await invoke<boolean>("has_available_update");
            if (hasAvailableUpdate.value) {
                availableUpdateVersion.value = cachedAvailableUpdateVersion;
                if (!cachedAvailableUpdateVersion || !cachedAvailableUpdateResource) {
                    void resolveAvailableUpdateVersion();
                }
            } else {
                void setAvailableUpdateResource(null);
            }
        } catch {
            void setAvailableUpdateResource(null);
        }
    };

    const resolveAvailableUpdateVersion = async () => {
        try {
            const update = await check();
            await setAvailableUpdateResource(update);
        } catch {
            // Keep last known availability when refresh check fails.
        }
    };

    const setupUpdateAvailabilityListener = async () => {
        try {
            unlistenUpdateAvailable = await listen<boolean>(
                "soia-update-available",
                (event) => {
                    hasAvailableUpdate.value = event.payload === true;
                    if (hasAvailableUpdate.value) {
                        availableUpdateVersion.value = cachedAvailableUpdateVersion;
                        if (!cachedAvailableUpdateVersion || !cachedAvailableUpdateResource) {
                            void resolveAvailableUpdateVersion();
                        }
                    } else {
                        void setAvailableUpdateResource(null);
                    }
                    if (!hasAvailableUpdate.value && !isUpdateBusyPhase(updateButtonPhase.value)) {
                        updateButtonPhase.value = "idle";
                    }
                },
            );
        } catch {
            unlistenUpdateAvailable = null;
        }
    };

    const shouldShowUpdateButton = computed(
        () =>
            hasAvailableUpdate.value ||
            isUpdateBusyPhase(updateButtonPhase.value) ||
            updateButtonPhase.value === "failed",
    );

    const isUpdateButtonDisabled = computed(
        () => isUpdateBusyPhase(updateButtonPhase.value),
    );

    const updateButtonText = computed(() => {
        if (
            updateButtonPhase.value === "checking" ||
            updateButtonPhase.value === "downloading"
        ) {
            return "Updating...";
        }
        if (updateButtonPhase.value === "installing") {
            return "Installing...";
        }
        if (updateButtonPhase.value === "failed") {
            return "Retry";
        }
        return "Update";
    });

    const isUpdateRetry = computed(
        () => updateButtonPhase.value === "failed",
    );

    const shouldShowUpdateStatus = computed(
        () =>
            updateButtonPhase.value === "checking" ||
            updateButtonPhase.value === "downloading" ||
            updateButtonPhase.value === "installing",
    );

    const updateDownloadPercent = computed(() => {
        if (updateButtonPhase.value !== "downloading") return null;
        const total = updateProgressTotalBytes.value;
        if (!total || total <= 0) return null;
        const ratio = updateProgressDownloadedBytes.value / total;
        const percent = Math.round(Math.min(1, Math.max(0, ratio)) * 100);
        return Number.isFinite(percent) ? percent : null;
    });

    const updateStatusText = computed(() => {
        if (updateButtonPhase.value === "checking") {
            return "Checking for update... 0%";
        }
        if (updateButtonPhase.value === "downloading") {
            const percent = updateDownloadPercent.value;
            const speed = updateDownloadSpeedBytesPerSec.value;
            const speedText =
                typeof speed === "number" && Number.isFinite(speed) && speed > 0
                    ? ` · ${formatByteSize(speed)}/s`
                    : "";
            if (typeof percent === "number") {
                return `Downloading update... ${percent}%${speedText}`;
            }
            return `Downloading update...${speedText}`;
        }
        if (updateButtonPhase.value === "installing") {
            return "Installing update...";
        }
        return "";
    });

    const shouldShowUpdateHint = computed(
        () => hasAvailableUpdate.value && !isUpdateBusyPhase(updateButtonPhase.value),
    );

    const updateHintText = computed(() => {
        const version = availableUpdateVersion.value?.trim();
        if (version) {
            return `Version ${version} is available!`;
        }
        return "New version is available!";
    });

    const openProjectReleases = async () => {
        try {
            await openUrl(PROJECT_RELEASES_URL);
            return;
        } catch {
            if (typeof window !== "undefined") {
                window.open(PROJECT_RELEASES_URL, "_blank", "noopener,noreferrer");
            }
        }
    };

    const installUpdate = async () => {
        if (isUpdateBusyPhase(updateButtonPhase.value)) return;
        if (updateWorkflowPromise) {
            await updateWorkflowPromise;
            return;
        }

        updateWorkflowPromise = (async () => {
            let allowEmbeddedInstall = true;
            try {
                allowEmbeddedInstall = await invoke<boolean>(
                    "should_use_embedded_update_install",
                );
            } catch {
                allowEmbeddedInstall = true;
            }
            if (!allowEmbeddedInstall) {
                updateButtonPhase.value = "idle";
                await openProjectReleases();
                return;
            }

            updateProgressTotalBytes.value = null;
            updateProgressDownloadedBytes.value = 0;
            updateDownloadSpeedBytesPerSec.value = null;
            updateLastProgressAtMs.value = null;
            updateLastProgressBytes.value = 0;

            const runDownloadAndInstall = async (update: Update) => {
                hasAvailableUpdate.value = true;
                availableUpdateVersion.value = update.version?.trim() || null;
                updateButtonPhase.value = "downloading";
                await update.download((event: DownloadEvent) => {
                    if (event.event === "Started") {
                        const contentLength = event.data.contentLength;
                        if (
                            typeof contentLength === "number" &&
                            Number.isFinite(contentLength) &&
                            contentLength > 0
                        ) {
                            updateProgressTotalBytes.value = contentLength;
                        } else {
                            updateProgressTotalBytes.value = null;
                        }
                        updateProgressDownloadedBytes.value = 0;
                        updateDownloadSpeedBytesPerSec.value = null;
                        updateLastProgressAtMs.value = Date.now();
                        updateLastProgressBytes.value = 0;
                        return;
                    }
                    if (event.event === "Progress") {
                        const chunkLength = event.data.chunkLength;
                        if (
                            typeof chunkLength === "number" &&
                            Number.isFinite(chunkLength) &&
                            chunkLength > 0
                        ) {
                            const nextValue = updateProgressDownloadedBytes.value + chunkLength;
                            const total = updateProgressTotalBytes.value;
                            updateProgressDownloadedBytes.value =
                                total && total > 0 ? Math.min(nextValue, total) : nextValue;
                        }

                        const now = Date.now();
                        const lastAt = updateLastProgressAtMs.value;
                        const lastBytes = updateLastProgressBytes.value;
                        if (lastAt && now > lastAt) {
                            const bytesDelta = updateProgressDownloadedBytes.value - lastBytes;
                            const timeDeltaSec = (now - lastAt) / 1000;
                            if (bytesDelta > 0 && timeDeltaSec > 0) {
                                const instantSpeed = bytesDelta / timeDeltaSec;
                                const previous = updateDownloadSpeedBytesPerSec.value;
                                updateDownloadSpeedBytesPerSec.value =
                                    previous && Number.isFinite(previous)
                                        ? previous * 0.7 + instantSpeed * 0.3
                                        : instantSpeed;
                            }
                        }
                        updateLastProgressAtMs.value = now;
                        updateLastProgressBytes.value = updateProgressDownloadedBytes.value;
                        return;
                    }
                    if (event.event === "Finished") {
                        const total = updateProgressTotalBytes.value;
                        if (total && total > 0) {
                            updateProgressDownloadedBytes.value = total;
                        }
                        updateDownloadSpeedBytesPerSec.value = null;
                    }
                });
                updateButtonPhase.value = "installing";
                await update.install();
                try {
                    const shouldRelaunch = await confirm(
                        translateSettingsText(
                            getSettingsLocale(),
                            "Update installed. Restart now to apply the new version?",
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
                    // Update already installed; ignore prompt failures.
                }
            };

            let primaryUpdate = consumeAvailableUpdateResource();
            let lastKnownVersion: string | null = primaryUpdate?.version?.trim() || null;

            try {
                if (!primaryUpdate) {
                    updateButtonPhase.value = "checking";
                    primaryUpdate = await check();
                }
                if (!primaryUpdate) {
                    await setAvailableUpdateResource(null);
                    updateButtonPhase.value = "idle";
                    return;
                }

                lastKnownVersion = primaryUpdate.version?.trim() || lastKnownVersion;

                try {
                    await runDownloadAndInstall(primaryUpdate);
                    closeUpdateResource(primaryUpdate);
                    await setAvailableUpdateResource(null);
                    updateButtonPhase.value = "idle";
                    return;
                } catch {
                    closeUpdateResource(primaryUpdate);
                    updateButtonPhase.value = "checking";
                    const fallbackUpdate = await check();
                    if (!fallbackUpdate) {
                        await setAvailableUpdateResource(null);
                        updateButtonPhase.value = "idle";
                        return;
                    }
                    lastKnownVersion = fallbackUpdate.version?.trim() || lastKnownVersion;
                    try {
                        await runDownloadAndInstall(fallbackUpdate);
                        closeUpdateResource(fallbackUpdate);
                        await setAvailableUpdateResource(null);
                        updateButtonPhase.value = "idle";
                        return;
                    } catch {
                        closeUpdateResource(fallbackUpdate);
                        hasAvailableUpdate.value = true;
                        availableUpdateVersion.value = lastKnownVersion;
                        updateButtonPhase.value = "failed";
                        return;
                    }
                }
            } catch {
                hasAvailableUpdate.value = true;
                availableUpdateVersion.value = lastKnownVersion;
                updateButtonPhase.value = "failed";
            }
        })();

        try {
            await updateWorkflowPromise;
        } finally {
            updateWorkflowPromise = null;
        }
    };

    const dispose = () => {
        unlistenUpdateAvailable?.();
        unlistenUpdateAvailable = null;
    };

    return {
        hasAvailableUpdate,
        availableUpdateVersion,
        updateButtonPhase,
        shouldShowUpdateButton,
        isUpdateButtonDisabled,
        updateButtonText,
        isUpdateRetry,
        shouldShowUpdateStatus,
        updateStatusText,
        shouldShowUpdateHint,
        updateHintText,
        loadUpdateAvailability,
        setupUpdateAvailabilityListener,
        installUpdate,
        dispose,
    };
};
