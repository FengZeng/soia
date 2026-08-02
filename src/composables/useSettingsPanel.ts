import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { confirm } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { SETTINGS_UPDATED_EVENT } from "../mock/settings";
import {
    createDebouncedUiStateSaver,
    clearOnlineSubtitleCache,
    factoryReset as invokeFactoryReset,
    loadUiState,
} from "./useUiStateStore";
import {
    useGeneralSettingsSection,
    type StoredSettingGroup,
    useRenderingSettingsSection,
    type StoredRenderingState,
    useAboutSection,
    useUpdateSection,
    useMediaAssociationSection,
    useAudioSettingsSection,
} from "./settings-sections";
import type { AudioSettings } from "../types/audio";

export const useSettingsPanel = () => {
    const isMacOS =
        typeof navigator !== "undefined" && /mac|darwin/i.test(navigator.userAgent);
    const isWindowsPlatform =
        typeof navigator !== "undefined" && /\bwindows\b/i.test(navigator.userAgent);

    const isLoading = ref(true);
    const isFactoryResetInProgress = ref(false);
    const isClearingOnlineSubtitleCache = ref(false);
    const onlineSubtitleCacheStatus = ref("");
    const uiStateSaver = createDebouncedUiStateSaver(300);

    const general = useGeneralSettingsSection(isWindowsPlatform);
    const rendering = useRenderingSettingsSection();
    const about = useAboutSection();
    const update = useUpdateSection();
    const mediaAssociation = useMediaAssociationSection(isMacOS);
    const audio = useAudioSettingsSection();
    const settingGroups = computed(() => [
        ...general.settingGroups.value,
        audio.settingGroup.value,
    ]);

    const emitSettingsUpdated = () => {
        if (typeof window === "undefined") return;
        window.dispatchEvent(
            new CustomEvent(SETTINGS_UPDATED_EVENT, {
                detail: {
                    groups: general.toPersistedGroups(),
                    rendering: rendering.toPersistedRendering(),
                    audio: audio.toPersistedAudio(),
                },
            }),
        );
    };

    const buildPersistedState = () => ({
        settings: {
            groups: general.toPersistedGroups(),
        },
        rendering: rendering.toPersistedRendering(),
    });

    const saveStateDebounced = () => {
        uiStateSaver.saveDebounced(buildPersistedState());
    };

    const saveStateImmediately = () => {
        uiStateSaver.flush(buildPersistedState());
    };

    const loadState = async () => {
        const stored = await loadUiState<{
            settings?: { groups?: StoredSettingGroup[] };
            rendering?: StoredRenderingState;
            audio?: AudioSettings;
        }>();

        await general.loadGeneralSettings(stored?.settings?.groups);
        await rendering.loadRenderingSettings(stored?.rendering);
        await audio.loadAudioSettings(stored?.audio);
        isLoading.value = false;
    };

    const resetAllSettings = () => {
        general.resetGeneralSettings();
        rendering.resetRenderingSettings();
        audio.resetAudioSettings();
    };

    const factoryReset = async () => {
        if (isFactoryResetInProgress.value) return;

        const confirmed = await confirm(
            "Factory reset will erase local history, playlists, settings, and network records. UUID will be kept. Continue?",
            {
                title: "Factory Reset",
                kind: "warning",
                okLabel: "Reset and Restart",
                cancelLabel: "Cancel",
            },
        ).catch(() => false);

        if (!confirmed) return;

        isFactoryResetInProgress.value = true;
        try {
            await invokeFactoryReset();
            await relaunch();
        } catch {
            isFactoryResetInProgress.value = false;
        }
    };

    const formatBytes = (bytes: number): string => {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    };

    const clearDownloadedSubtitles = async () => {
        if (isClearingOnlineSubtitleCache.value) return;
        isClearingOnlineSubtitleCache.value = true;
        onlineSubtitleCacheStatus.value = "";
        try {
            const result = await clearOnlineSubtitleCache();
            onlineSubtitleCacheStatus.value =
                result.removedFiles === 0
                    ? "No downloaded subtitles to clear."
                    : `Cleared ${result.removedFiles} file${
                          result.removedFiles === 1 ? "" : "s"
                      } (${formatBytes(result.removedBytes)}).`;
        } catch (error) {
            onlineSubtitleCacheStatus.value = String(error);
        } finally {
            isClearingOnlineSubtitleCache.value = false;
        }
    };

    onMounted(() => {
        void loadState();
        void about.loadRuntimeVersions();
        void mediaAssociation.refreshMediaAssociationStatus();
        void update.loadUpdateAvailability();
        void update.setupUpdateAvailabilityListener();
    });

    onUnmounted(() => {
        saveStateImmediately();
        general.dispose();
        rendering.dispose();
        update.dispose();
    });

    watch(
        general.settingGroups,
        () => {
            if (isLoading.value) return;
            general.applySectionSideEffects();
            saveStateDebounced();
            emitSettingsUpdated();
        },
        { deep: true },
    );

    watch(
        audio.settingGroup,
        () => {
            if (isLoading.value) return;
            void audio.applySectionSideEffects();
            saveStateImmediately();
            emitSettingsUpdated();
        },
        { deep: true },
    );

    watch(
        [
            rendering.renderingMode,
            rendering.selectedShaderFiles,
            rendering.activeShaderFiles,
        ],
        () => {
            if (isLoading.value) return;
            rendering.scheduleApplyRenderingOptions();
            saveStateImmediately();
            emitSettingsUpdated();
        },
        { deep: true },
    );

    return {
        settingGroups,
        audioOutputStatus: audio.outputStatus,
        audioOutputError: audio.outputError,
        retryAudioOutput: audio.retryOutput,
        runtimeVersions: about.runtimeVersions,
        mediaAssociationStatus: mediaAssociation.mediaAssociationStatus,
        canManageMediaAssociation: mediaAssociation.canManageMediaAssociation,
        shouldShowSetDefaultMediaButton:
            mediaAssociation.shouldShowSetDefaultMediaButton,
        isSetDefaultButtonDisabled: mediaAssociation.isSetDefaultButtonDisabled,
        isSetDefaultButtonLoading: mediaAssociation.isSetDefaultButtonLoading,
        setDefaultButtonText: mediaAssociation.setDefaultButtonText,
        isSetDefaultSuccess: mediaAssociation.isSetDefaultSuccess,
        shouldShowUpdateButton: update.shouldShowUpdateButton,
        isUpdateButtonDisabled: update.isUpdateButtonDisabled,
        updateButtonText: update.updateButtonText,
        isUpdateRetry: update.isUpdateRetry,
        shouldShowUpdateStatus: update.shouldShowUpdateStatus,
        updateStatusText: update.updateStatusText,
        shouldShowUpdateHint: update.shouldShowUpdateHint,
        updateHintText: update.updateHintText,
        mediaAssociationSummary: mediaAssociation.mediaAssociationSummary,
        mediaAssociationMessage: mediaAssociation.mediaAssociationMessage,
        isCheckingMediaAssociation: mediaAssociation.isCheckingMediaAssociation,
        isApplyingMediaAssociation: mediaAssociation.isApplyingMediaAssociation,
        refreshMediaAssociationStatus: mediaAssociation.refreshMediaAssociationStatus,
        setMediaAssociationToSoia: mediaAssociation.setMediaAssociationToSoia,
        installUpdate: update.installUpdate,
        openProjectGithub: about.openProjectGithub,
        openSubreddit: about.openSubreddit,
        resetAllSettings,
        factoryReset,
        isFactoryResetInProgress,
        clearDownloadedSubtitles,
        isClearingOnlineSubtitleCache,
        onlineSubtitleCacheStatus,
        browseForPath: general.browseForPath,
        browseForCustomShaders: rendering.browseForCustomShaders,
        selectedShaderFiles: rendering.selectedShaderFiles,
        activeShaderFiles: rendering.activeShaderFiles,
        unavailableShaderFiles: rendering.unavailableShaderFiles,
        multiShaderEnabled: rendering.multiShaderEnabled,
        renderingMode: rendering.renderingMode,
        setShaderEnabled: rendering.setShaderEnabled,
        setMultiShaderEnabled: rendering.setMultiShaderEnabled,
        setRenderingMode: rendering.setRenderingMode,
        removeShaderFromList: rendering.removeShaderFromList,
        clearShaders: rendering.clearShaders,
        isFixedLogPathItem: general.isFixedLogPathItem,
        isLoading,
    };
};
