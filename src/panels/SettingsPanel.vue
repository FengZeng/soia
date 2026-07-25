<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsPanel } from "../composables/useSettingsPanel";
import {
    useRemoteControlQrDialog,
    type RemoteControlStatus,
} from "../composables/useRemoteControlQrDialog";
import RemoteControlQrDialog from "../components/RemoteControlQrDialog.vue";
import { getPathDisplayName } from "../utils/getPathDisplayName";
import {
    ENABLE_COMPACT_MODE_SETTING_LABEL,
    ONLINE_SUBTITLES_SETTING_GROUP_TITLE,
    OPENSUBTITLES_API_KEY_SETTING_LABEL,
    OPENSUBTITLES_ENABLED_SETTING_LABEL,
    OPENSUBTITLES_LANGUAGES_SETTING_LABEL,
    SUBSOURCE_API_KEY_SETTING_LABEL,
    SUBSOURCE_ENABLED_SETTING_LABEL,
    SUBSOURCE_LANGUAGES_SETTING_LABEL,
    WALLPAPER_MODE_SETTING_LABEL,
    type SettingItem,
    type SettingGroup,
} from "../mock/settings";

const {
    settingGroups,
    runtimeVersions,
    shouldShowSetDefaultMediaButton,
    isSetDefaultButtonDisabled,
    isSetDefaultButtonLoading,
    setDefaultButtonText,
    isSetDefaultSuccess,
    shouldShowUpdateButton,
    isUpdateButtonDisabled,
    updateButtonText,
    isUpdateRetry,
    shouldShowUpdateStatus,
    updateStatusText,
    shouldShowUpdateHint,
    updateHintText,
    openProjectGithub,
    openSubreddit,
    isApplyingMediaAssociation,
    setMediaAssociationToSoia,
    installUpdate,
    resetAllSettings,
    factoryReset,
    isFactoryResetInProgress,
    clearDownloadedSubtitles,
    isClearingOnlineSubtitleCache,
    onlineSubtitleCacheStatus,
    browseForPath,
    browseForCustomShaders,
    selectedShaderFiles,
    activeShaderFiles,
    unavailableShaderFiles,
    multiShaderEnabled,
    renderingMode,
    setShaderEnabled,
    setMultiShaderEnabled,
    setRenderingMode,
    removeShaderFromList,
    clearShaders,
    isFixedLogPathItem,
    isLoading,
} = useSettingsPanel();

const isLinuxPlatform =
    typeof navigator !== "undefined" && /\blinux\b/i.test(navigator.userAgent);
const isWindowsPlatform =
    typeof navigator !== "undefined" && /\bwindows\b/i.test(navigator.userAgent);

const shouldShowSettingItem = (item: SettingItem): boolean =>
    !(
        (isLinuxPlatform && item.label === ENABLE_COMPACT_MODE_SETTING_LABEL) ||
        (!isWindowsPlatform && item.label === WALLPAPER_MODE_SETTING_LABEL)
    );

const visibleItems = (group: SettingGroup): SettingItem[] =>
    group.items.filter(shouldShowSettingItem);

const onlineSubtitleTabs = [
    {
        id: "opensubtitles",
        label: "OpenSubtitles",
        itemLabels: [
            OPENSUBTITLES_ENABLED_SETTING_LABEL,
            OPENSUBTITLES_API_KEY_SETTING_LABEL,
            OPENSUBTITLES_LANGUAGES_SETTING_LABEL,
        ],
    },
    {
        id: "subsource",
        label: "SubSource",
        itemLabels: [
            SUBSOURCE_ENABLED_SETTING_LABEL,
            SUBSOURCE_API_KEY_SETTING_LABEL,
            SUBSOURCE_LANGUAGES_SETTING_LABEL,
        ],
    },
] as const;
const activeOnlineSubtitleTab = ref<(typeof onlineSubtitleTabs)[number]["id"]>(
    "opensubtitles",
);

const remoteControlError = ref("");
const isLoadingRemoteControl = ref(false);
const remoteControlStatus = ref<RemoteControlStatus | null>(null);
const {
    remoteControlInfo,
    isRemoteQrOpen,
    remoteQrSecondsRemaining,
    showRemoteControlQr,
    closeRemoteQrDialog,
} = useRemoteControlQrDialog((status) => {
    remoteControlStatus.value = status;
});

const showRemoteControl = async () => {
    isLoadingRemoteControl.value = true;
    remoteControlError.value = "";
    try {
        await showRemoteControlQr();
    } catch (error) {
        remoteControlError.value = String(error);
    } finally {
        isLoadingRemoteControl.value = false;
    }
};

const refreshRemoteControlStatus = async () => {
    remoteControlStatus.value = await invoke<RemoteControlStatus>("get_remote_control_status");
};

const setRemoteControlEnabled = async (enabled: boolean) => {
    isLoadingRemoteControl.value = true;
    try {
        remoteControlStatus.value = await invoke<RemoteControlStatus>("set_remote_control_enabled", { enabled });
        closeRemoteQrDialog();
    } catch (error) {
        remoteControlError.value = String(error);
    } finally {
        isLoadingRemoteControl.value = false;
    }
};

const onRemoteControlToggle = (event: Event) => {
    void setRemoteControlEnabled((event.target as HTMLInputElement).checked);
};

const disconnectRemoteControlDevices = async () => {
    try {
        remoteControlStatus.value = await invoke<RemoteControlStatus>("disconnect_remote_control_devices");
    } catch (error) {
        remoteControlError.value = String(error);
    }
};

const onRemoteQrKeydown = (event: KeyboardEvent) => {
    if (event.key === "Escape" && isRemoteQrOpen.value) {
        closeRemoteQrDialog();
    }
};

const visibleOnlineSubtitleItems = (group: SettingGroup): SettingItem[] => {
    const activeTab =
        onlineSubtitleTabs.find((tab) => tab.id === activeOnlineSubtitleTab.value) ??
        onlineSubtitleTabs[0];
    const labels = new Set<string>(activeTab.itemLabels);
    return visibleItems(group).filter((item) => labels.has(item.label));
};

const displayedGroupItems = (group: SettingGroup): SettingItem[] =>
    group.title === ONLINE_SUBTITLES_SETTING_GROUP_TITLE
        ? visibleOnlineSubtitleItems(group)
        : visibleItems(group);

const shouldShowGroup = (group: SettingGroup): boolean =>
    displayedGroupItems(group).length > 0;

const getShaderDisplayName = (path: string): string => {
    const name = getPathDisplayName(path, path);
    return name.replace(/\.glsl$/i, "");
};

const isShaderUnavailable = (path: string): boolean =>
    unavailableShaderFiles.value.includes(path);

const getActiveShaderOrder = (path: string): number | null => {
    const index = activeShaderFiles.value.indexOf(path);
    return index >= 0 ? index + 1 : null;
};

const toggleShaderEnabled = (path: string) => {
    const nextEnabled = !activeShaderFiles.value.includes(path);
    setShaderEnabled(path, nextEnabled);
};

const SHADER_COLLAPSED_VISIBLE_COUNT = 4;
const isShaderListExpanded = ref(false);
const openSelectKey = ref<string | null>(null);
const activeSelectOptionIndex = ref<number>(0);
const selectTriggerRefs = new Map<string, HTMLElement>();
const selectMenuStyle = ref<Record<string, string>>({});

const getSelectKey = (groupTitle: string, itemLabel: string): string =>
    `${groupTitle}::${itemLabel}`;

const isSelectItem = (
    item: SettingItem,
): item is Extract<SettingItem, { type: "select" }> => item.type === "select";

const getSelectOptionIndex = (item: SettingItem): number => {
    if (!isSelectItem(item)) {
        return 0;
    }
    const index = item.options.indexOf(item.value);
    return index >= 0 ? index : 0;
};

const clampOptionIndex = (item: SettingItem, index: number): number => {
    if (!isSelectItem(item)) {
        return 0;
    }
    if (!item.options.length) {
        return 0;
    }
    if (index < 0) {
        return item.options.length - 1;
    }
    if (index >= item.options.length) {
        return 0;
    }
    return index;
};

const openSelect = (key: string, item: SettingItem) => {
    openSelectKey.value = key;
    activeSelectOptionIndex.value = getSelectOptionIndex(item);
    nextTick(() => {
        updateOpenSelectMenuPosition();
    });
};

const closeSelect = () => {
    openSelectKey.value = null;
};

const toggleSelect = (key: string, item: SettingItem) => {
    if (openSelectKey.value === key) {
        closeSelect();
        return;
    }
    openSelect(key, item);
};

const isSelectOpen = (key: string): boolean => openSelectKey.value === key;

const setActiveSelectOption = (item: SettingItem, step: number) => {
    activeSelectOptionIndex.value = clampOptionIndex(
        item,
        activeSelectOptionIndex.value + step,
    );
};

const chooseSelectOption = (item: SettingItem, option: string) => {
    if (!isSelectItem(item)) {
        return;
    }
    item.value = option;
    closeSelect();
};

const setSelectTriggerRef = (key: string, element: HTMLElement | null) => {
    if (!element) {
        selectTriggerRefs.delete(key);
        return;
    }
    selectTriggerRefs.set(key, element);
};

const getOpenSelectMenuStyle = (): Record<string, string> => selectMenuStyle.value;

const updateOpenSelectMenuPosition = () => {
    if (!openSelectKey.value) {
        return;
    }
    const trigger = selectTriggerRefs.get(openSelectKey.value);
    if (!trigger) {
        return;
    }
    const rect = trigger.getBoundingClientRect();
    const viewportHeight = window.innerHeight;
    const spaceAbove = rect.top;
    const spaceBelow = viewportHeight - rect.bottom;
    const estimatedMenuHeight = 240;
    const shouldOpenTop =
        spaceBelow < estimatedMenuHeight && spaceAbove > spaceBelow;
    const gap = 6;
    const maxHeight = Math.max(
        120,
        Math.min(320, shouldOpenTop ? spaceAbove - 10 : spaceBelow - 10),
    );
    const triggerStyles = getComputedStyle(trigger);
    const menuThemeVars: Record<string, string> = {
        "--panel-select-card-border": triggerStyles
            .getPropertyValue("--panel-select-card-border")
            .trim(),
        "--panel-select-card-text": triggerStyles
            .getPropertyValue("--panel-select-card-text")
            .trim(),
        "--panel-select-card-hover-bg": triggerStyles
            .getPropertyValue("--panel-select-card-hover-bg")
            .trim(),
        "--panel-select-card-focus-bg": triggerStyles
            .getPropertyValue("--panel-select-card-focus-bg")
            .trim(),
        "--panel-select-card-focus-border": triggerStyles
            .getPropertyValue("--panel-select-card-focus-border")
            .trim(),
        "--panel-select-menu-bg": triggerStyles
            .getPropertyValue("--panel-select-menu-bg")
            .trim(),
        "--panel-select-menu-border": triggerStyles
            .getPropertyValue("--panel-select-menu-border")
            .trim(),
        "--panel-select-menu-hover-bg": triggerStyles
            .getPropertyValue("--panel-select-menu-hover-bg")
            .trim(),
        "--panel-select-menu-selected-bg": triggerStyles
            .getPropertyValue("--panel-select-menu-selected-bg")
            .trim(),
        "--panel-select-menu-selected-border": triggerStyles
            .getPropertyValue("--panel-select-menu-selected-border")
            .trim(),
    };

    selectMenuStyle.value = shouldOpenTop
        ? {
              ...menuThemeVars,
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              bottom: `${viewportHeight - rect.top + gap}px`,
              maxHeight: `${maxHeight}px`,
          }
        : {
              ...menuThemeVars,
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              top: `${rect.bottom + gap}px`,
              maxHeight: `${maxHeight}px`,
          };
};

const onSelectTriggerKeydown = (
    event: KeyboardEvent,
    key: string,
    item: SettingItem,
) => {
    if (!isSelectItem(item)) {
        return;
    }
    if (!item.options.length) {
        return;
    }

    if (event.key === "ArrowDown") {
        event.preventDefault();
        if (!isSelectOpen(key)) {
            openSelect(key, item);
            return;
        }
        setActiveSelectOption(item, 1);
        return;
    }

    if (event.key === "ArrowUp") {
        event.preventDefault();
        if (!isSelectOpen(key)) {
            openSelect(key, item);
            return;
        }
        setActiveSelectOption(item, -1);
        return;
    }

    if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        if (!isSelectOpen(key)) {
            openSelect(key, item);
            return;
        }
        const nextOption = item.options[activeSelectOptionIndex.value];
        if (nextOption !== undefined) {
            chooseSelectOption(item, nextOption);
        }
        return;
    }

    if (event.key === "Escape") {
        if (isSelectOpen(key)) {
            event.preventDefault();
            closeSelect();
        }
    }
};

const onDocumentPointerDown = (event: PointerEvent) => {
    if (!openSelectKey.value) {
        return;
    }
    const target = event.target as HTMLElement | null;
    if (target?.closest(".panel__custom-select, .panel__custom-select-menu")) {
        return;
    }
    closeSelect();
};
const shouldShowShaderListCollapseToggle = computed(
    () => selectedShaderFiles.value.length > SHADER_COLLAPSED_VISIBLE_COUNT,
);
const shouldShowMultiShaderToggle = computed(
    () => selectedShaderFiles.value.length > 1,
);
const visibleShaderFiles = computed(() => {
    if (
        shouldShowShaderListCollapseToggle.value &&
        !isShaderListExpanded.value
    ) {
        return selectedShaderFiles.value.slice(0, SHADER_COLLAPSED_VISIBLE_COUNT);
    }
    return selectedShaderFiles.value;
});

const isNormalRenderingMode = computed(() => renderingMode.value === "normal");
const isAnimeModeRenderingMode = computed(
    () => renderingMode.value === "animeMode",
);
const hasEnabledShaderInCurrentMode = computed(
    () => activeShaderFiles.value.length > 0,
);
const shaderModeHintText = computed(() =>
    isAnimeModeRenderingMode.value
        ? "Anime Mode: Auto-detect anime videos and apply shaders only for anime."
        : "General Mode: Selected shaders will be applied to all videos.",
);

watch(
    selectedShaderFiles,
    (next) => {
        if (!next.length) {
            isShaderListExpanded.value = false;
        }
    },
    { deep: true },
);

onMounted(() => {
    void refreshRemoteControlStatus().catch((error: unknown) => { remoteControlError.value = String(error); });
    document.addEventListener("pointerdown", onDocumentPointerDown);
    document.addEventListener("scroll", updateOpenSelectMenuPosition, true);
    window.addEventListener("resize", updateOpenSelectMenuPosition);
    window.addEventListener("keydown", onRemoteQrKeydown);
});

onBeforeUnmount(() => {
    document.removeEventListener("pointerdown", onDocumentPointerDown);
    document.removeEventListener("scroll", updateOpenSelectMenuPosition, true);
    window.removeEventListener("resize", updateOpenSelectMenuPosition);
    window.removeEventListener("keydown", onRemoteQrKeydown);
    closeRemoteQrDialog();
});
</script>

<template>
    <div class="panel panel--settings">
        <div class="panel__header">
            <div class="panel__title">Settings</div>
            <div class="panel__header-actions">
                <div v-if="shouldShowUpdateStatus" class="panel__update-status" aria-live="polite">
                    <span class="panel__spinner" aria-hidden="true"></span>
                    <div class="panel__update-status-text">{{ updateStatusText }}</div>
                </div>
                <div v-if="shouldShowUpdateButton" class="panel__update-action-wrap">
                    <span
                        v-if="shouldShowUpdateHint"
                        class="panel__update-hint"
                    >
                        {{ updateHintText }}
                    </span>
                    <button
                        class="panel__action panel__action--glow panel__header-action panel__header-action--compact"
                        type="button"
                        :disabled="isUpdateButtonDisabled"
                        @click="installUpdate"
                    >
                        <span
                            v-if="isUpdateRetry"
                            class="panel__status-icon panel__status-icon--failed"
                            aria-hidden="true"
                        >
                            <svg class="panel__status-icon-svg" viewBox="0 0 20 20">
                                <path d="M5.5 5.5l9 9M14.5 5.5l-9 9" />
                            </svg>
                        </span>
                        <span>{{ updateButtonText }}</span>
                    </button>
                </div>
                <button
                    v-if="shouldShowSetDefaultMediaButton"
                    class="panel__action panel__action--glow panel__header-action"
                    type="button"
                    :disabled="isSetDefaultButtonDisabled || isApplyingMediaAssociation"
                    @click="setMediaAssociationToSoia"
                >
                    <span
                        v-if="isSetDefaultSuccess"
                        class="panel__status-icon panel__status-icon--success"
                        aria-hidden="true"
                    >
                        <svg class="panel__status-icon-svg" viewBox="0 0 20 20">
                            <path d="M4.5 10.5l3.4 3.4 7.6-7.8" />
                        </svg>
                    </span>
                    <span>{{ setDefaultButtonText }}</span>
                    <span v-if="isSetDefaultButtonLoading" class="panel__loading-dots" aria-hidden="true">
                        <span class="panel__loading-dot"></span>
                        <span class="panel__loading-dot"></span>
                        <span class="panel__loading-dot"></span>
                    </span>
                </button>
                <button class="panel__reset" type="button" @click="resetAllSettings">
                    Reset
                </button>
            </div>
        </div>
        <section
            v-if="remoteControlStatus || remoteControlError"
            class="panel__section panel__remote-control"
        >
            <div class="panel__remote-shell" :class="{ 'panel__remote-shell--disabled': remoteControlStatus && !remoteControlStatus.enabled }">
                <div class="panel__remote-header">
                    <div class="panel__remote-heading">
                        <span class="panel__remote-icon" aria-hidden="true">
                            <svg viewBox="0 0 24 24" fill="none">
                                <rect x="6.5" y="2.5" width="11" height="19" rx="2.5" />
                                <path d="M10 5.5h4M10.5 18.5h3" />
                            </svg>
                        </span>
                        <span class="panel__remote-heading-text">
                            <span class="panel__remote-title">Remote Controller</span>
                            <span class="panel__remote-copy">Control playback from a web browser on the same local network</span>
                        </span>
                    </div>
                    <label class="panel__toggle panel__remote-toggle">
                        <input
                            class="panel__toggle-input"
                            type="checkbox"
                            :checked="remoteControlStatus?.enabled === true"
                            :disabled="isLoadingRemoteControl || !remoteControlStatus"
                            aria-label="Enable Remote Controller"
                            @change="onRemoteControlToggle"
                        />
                        <span class="panel__toggle-track"><span class="panel__toggle-thumb"></span></span>
                    </label>
                </div>

                <div v-if="remoteControlStatus?.enabled" class="panel__remote-toolbar">
                    <div class="panel__remote-state">
                        <span class="panel__remote-state-dot" :class="{ 'panel__remote-state-dot--online': remoteControlStatus?.enabled }"></span>
                        <span>{{ remoteControlStatus?.enabled ? "Available on local network" : "Remote access is off" }}</span>
                        <span v-if="remoteControlStatus?.connectedDevices" class="panel__remote-device-count">
                            {{ remoteControlStatus.connectedDevices }} paired
                        </span>
                    </div>
                    <div class="panel__remote-actions">
                        <button
                            v-if="remoteControlStatus?.connectedDevices"
                            class="panel__action panel__action--ghost panel__action--compact"
                            type="button"
                            @click="disconnectRemoteControlDevices"
                        >
                            Disconnect all
                        </button>
                        <button
                            class="panel__action panel__action--accent panel__remote-pair-button"
                            type="button"
                            :disabled="isLoadingRemoteControl || !remoteControlStatus?.enabled"
                            @click="showRemoteControl"
                        >
                            {{ isLoadingRemoteControl ? "Preparing…" : "Show QR Code" }}
                        </button>
                    </div>
                </div>

                <p v-if="remoteControlError" class="panel__remote-error">{{ remoteControlError }}</p>
            </div>
        </section>
        <RemoteControlQrDialog
            :open="isRemoteQrOpen"
            :info="remoteControlInfo"
            :seconds-remaining="remoteQrSecondsRemaining"
            @close="closeRemoteQrDialog"
        />
        <div v-if="isLoading" class="panel__skeleton">
            <div class="panel__skeleton-row"></div>
            <div class="panel__skeleton-row"></div>
            <div class="panel__skeleton-row"></div>
        </div>
        <div v-if="!settingGroups.length" class="panel__empty">
            <div class="panel__empty-title">No settings yet</div>
            <div class="panel__empty-body">
                Add configuration options to start customizing playback.
            </div>
        </div>
        <div v-else class="panel__stack">
            <div
                v-for="group in settingGroups"
                :key="group.title"
                class="panel__section"
                v-show="shouldShowGroup(group)"
            >
                <div class="panel__subtitle panel__subtitle--large">
                    {{ group.title }}
                </div>
                <div
                    v-if="group.title === ONLINE_SUBTITLES_SETTING_GROUP_TITLE"
                    class="panel__section-toolbar"
                >
                    <div
                        class="panel__tabs"
                        role="tablist"
                        aria-label="Online subtitle providers"
                    >
                        <button
                            v-for="tab in onlineSubtitleTabs"
                            :key="tab.id"
                            class="panel__tab"
                            :class="{
                                'panel__tab--active': activeOnlineSubtitleTab === tab.id,
                            }"
                            type="button"
                            role="tab"
                            :aria-selected="activeOnlineSubtitleTab === tab.id"
                            @click="activeOnlineSubtitleTab = tab.id"
                        >
                            {{ tab.label }}
                        </button>
                    </div>
                    <div class="panel__toolbar-actions">
                        <div
                            v-if="onlineSubtitleCacheStatus"
                            class="panel__cache-status"
                            role="status"
                        >
                            {{ onlineSubtitleCacheStatus }}
                        </div>
                        <button
                            class="panel__action panel__action--ghost panel__action--compact"
                            type="button"
                            :disabled="isClearingOnlineSubtitleCache"
                            @click="clearDownloadedSubtitles"
                        >
                            {{ isClearingOnlineSubtitleCache ? "Clearing..." : "Clear Cache" }}
                        </button>
                    </div>
                </div>
                <div class="panel__table panel__table--card">
                    <div
                        v-for="item in displayedGroupItems(group)"
                        :key="item.label"
                        class="panel__row panel__row--card"
                    >
                        <div class="panel__card-text">
                            <div class="panel__card-title">
                                {{ item.displayLabel ?? item.label }}
                            </div>
                        </div>
                        <div class="panel__control panel__control--card">
                            <template v-if="item.type === 'path'">
                                <div class="panel__path-field">
                                    <div class="panel__path-control">
                                        <template v-if="isFixedLogPathItem(item)">
                                            <span class="panel__value-text panel__path-text panel__path-text--log">
                                                {{ item.value || item.placeholder || "Unavailable" }}
                                            </span>
                                        </template>
                                        <template v-else>
                                            <input
                                                v-model="item.value"
                                                class="panel__input panel__input--path"
                                                :class="{ 'panel__input--invalid': item.validationMessage }"
                                                type="text"
                                                :placeholder="item.placeholder"
                                                :aria-invalid="Boolean(item.validationMessage)"
                                            />
                                        </template>
                                        <button
                                            class="panel__action panel__action--ghost panel__action--icon panel__path-action"
                                            type="button"
                                            :title="isFixedLogPathItem(item) ? 'Open Folder' : item.browseTitle ?? 'Browse'"
                                            :aria-label="isFixedLogPathItem(item) ? 'Open Folder' : item.browseTitle ?? 'Browse'"
                                            @click="browseForPath(item)"
                                        >
                                            <svg
                                                class="panel__action-icon panel__path-action-icon"
                                                xmlns="http://www.w3.org/2000/svg"
                                                viewBox="0 0 24 24"
                                                fill="none"
                                                stroke="currentColor"
                                                stroke-width="2"
                                                stroke-linecap="round"
                                                stroke-linejoin="round"
                                            >
                                                <path
                                                    d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"
                                                ></path>
                                            </svg>
                                        </button>
                                    </div>
                                    <p
                                        v-if="item.validationMessage"
                                        class="panel__validation"
                                        role="status"
                                    >
                                        <svg
                                            class="panel__validation-icon"
                                            xmlns="http://www.w3.org/2000/svg"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />
                                            <path d="M12 9v4" />
                                            <path d="M12 17h.01" />
                                        </svg>
                                        <span>{{ item.validationMessage }}</span>
                                    </p>
                                </div>
                            </template>
                            <template v-else-if="item.type === 'text'">
                                <div class="panel__path-field">
                                    <input
                                        v-model="item.value"
                                        class="panel__input panel__input--path"
                                        :class="{ 'panel__input--invalid': item.validationMessage }"
                                        type="text"
                                        :placeholder="item.placeholder"
                                        :aria-invalid="Boolean(item.validationMessage)"
                                    />
                                    <p
                                        v-if="item.validationMessage"
                                        class="panel__validation"
                                        role="status"
                                    >
                                        <svg
                                            class="panel__validation-icon"
                                            xmlns="http://www.w3.org/2000/svg"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />
                                            <path d="M12 9v4" />
                                            <path d="M12 17h.01" />
                                        </svg>
                                        <span>{{ item.validationMessage }}</span>
                                    </p>
                                </div>
                            </template>
                            <template v-else-if="item.type === 'slider'">
                                <div class="panel__slider">
                                    <input
                                        v-model="item.value"
                                        class="panel__slider-input"
                                        type="range"
                                        :min="item.min"
                                        :max="item.max"
                                        :step="item.step"
                                        :style="{
                                            '--slider-value': `${
                                                ((Number(item.value) - item.min) /
                                                    (item.max - item.min)) *
                                                100
                                            }%`,
                                        }"
                                    />
                                    <div class="panel__slider-value">
                                        {{ item.value }}{{ item.unit }}
                                    </div>
                                </div>
                            </template>
                            <template v-else-if="item.type === 'toggle'">
                                <label class="panel__toggle">
                                    <input
                                        class="panel__toggle-input"
                                        type="checkbox"
                                        :checked="item.value === item.onValue"
                                        @change="
                                            item.value = ($event.target as HTMLInputElement).checked
                                                ? item.onValue
                                                : item.offValue
                                        "
                                    />
                                    <span class="panel__toggle-track">
                                        <span class="panel__toggle-thumb"></span>
                                    </span>
                                </label>
                            </template>
                            <template v-else>
                                <div
                                    class="panel__custom-select"
                                    :class="{
                                        'panel__custom-select--open': isSelectOpen(
                                            getSelectKey(group.title, item.label),
                                        ),
                                    }"
                                >
                                    <button
                                        type="button"
                                        class="panel__custom-select-trigger"
                                        :ref="
                                            (el) =>
                                                setSelectTriggerRef(
                                                    getSelectKey(group.title, item.label),
                                                    el as HTMLElement | null,
                                                )
                                        "
                                        :aria-expanded="
                                            isSelectOpen(getSelectKey(group.title, item.label))
                                        "
                                        aria-haspopup="listbox"
                                        @click="
                                            toggleSelect(
                                                getSelectKey(group.title, item.label),
                                                item,
                                            )
                                        "
                                        @keydown="
                                            onSelectTriggerKeydown(
                                                $event,
                                                getSelectKey(group.title, item.label),
                                                item,
                                            )
                                        "
                                    >
                                        <span class="panel__custom-select-value">
                                            {{ item.value }}
                                        </span>
                                        <span class="panel__custom-select-arrow" aria-hidden="true">
                                            <svg viewBox="0 0 12 12">
                                                <path d="M2.25 4.5L6 8.25L9.75 4.5" />
                                            </svg>
                                        </span>
                                    </button>
                                    <Teleport to="body">
                                        <div
                                            v-if="isSelectOpen(getSelectKey(group.title, item.label))"
                                            class="panel__custom-select-menu"
                                            :style="getOpenSelectMenuStyle()"
                                            role="listbox"
                                            :aria-label="item.displayLabel ?? item.label"
                                        >
                                            <button
                                                v-for="(option, optionIndex) in item.options"
                                                :key="option"
                                                type="button"
                                                class="panel__custom-select-option"
                                                :class="{
                                                    'panel__custom-select-option--selected':
                                                        option === item.value,
                                                    'panel__custom-select-option--active':
                                                        optionIndex === activeSelectOptionIndex,
                                                }"
                                                role="option"
                                                :aria-selected="option === item.value"
                                                @mouseenter="activeSelectOptionIndex = optionIndex"
                                                @click="chooseSelectOption(item, option)"
                                            >
                                                {{ option }}
                                            </button>
                                        </div>
                                    </Teleport>
                                </div>
                            </template>
                        </div>
                    </div>
                </div>
                <template v-if="group.title === 'Playback'">
                    <div class="panel__subtitle panel__subtitle--large">
                        Rendering
                    </div>
                    <div class="panel__table panel__table--card">
                        <div class="panel__row panel__row--card panel__row--shader-header">
                            <div class="panel__card-text">
                                <div class="panel__card-title">Custom Shader</div>
                                <div class="panel__card-subtitle">
                                    Select one or more <code>.glsl</code> shader files.
                                </div>
                            </div>
                            <div class="panel__control panel__control--card panel__control--stack">
                                <div class="panel__actions panel__actions--inline panel__actions--shader">
                                    <button
                                        class="panel__action panel__action--ghost panel__action--compact"
                                        type="button"
                                        @click="browseForCustomShaders"
                                    >
                                        Add Shaders
                                    </button>
                                    <button
                                        class="panel__action panel__action--ghost panel__action--compact"
                                        type="button"
                                        :disabled="!selectedShaderFiles.length"
                                        @click="clearShaders"
                                    >
                                        Clear
                                    </button>
                                </div>
                            </div>
                        </div>
                        <div class="panel__row panel__row--card panel__row--stacked">
                            <div class="panel__shader-list-wrap">
                                <div class="panel__shader-mode-hint">
                                    {{ shaderModeHintText }}
                                </div>
                                <div
                                    v-if="!selectedShaderFiles.length"
                                    class="panel__shader-empty"
                                >
                                    No shader files selected.
                                </div>
                                <div v-else class="panel__shader-list">
                                    <div
                                        v-for="shaderPath in visibleShaderFiles"
                                        :key="shaderPath"
                                        class="panel__shader-item"
                                        :class="{
                                            'panel__shader-item--active':
                                                getActiveShaderOrder(shaderPath) !== null,
                                            'panel__shader-item--unavailable':
                                                isShaderUnavailable(shaderPath),
                                        }"
                                        role="checkbox"
                                        :aria-checked="
                                            getActiveShaderOrder(shaderPath) !== null
                                        "
                                        :aria-disabled="
                                            isShaderUnavailable(shaderPath)
                                        "
                                        :tabindex="
                                            isShaderUnavailable(shaderPath) ? -1 : 0
                                        "
                                        :title="
                                            isShaderUnavailable(shaderPath)
                                                ? `File not found: ${shaderPath}`
                                                : shaderPath
                                        "
                                        @click="toggleShaderEnabled(shaderPath)"
                                        @keydown.enter.prevent="
                                            toggleShaderEnabled(shaderPath)
                                        "
                                        @keydown.space.prevent="
                                            toggleShaderEnabled(shaderPath)
                                        "
                                    >
                                        <span
                                            v-if="multiShaderEnabled"
                                            class="panel__shader-select"
                                            :class="{
                                                'panel__shader-select--active':
                                                    getActiveShaderOrder(shaderPath) !== null,
                                                'panel__shader-select--unavailable':
                                                    isShaderUnavailable(shaderPath),
                                            }"
                                            :title="
                                                getActiveShaderOrder(shaderPath) !== null
                                                    ? `Shader order ${getActiveShaderOrder(shaderPath)}`
                                                    : 'Select shader'
                                            "
                                        >
                                            {{ getActiveShaderOrder(shaderPath) ?? "" }}
                                        </span>
                                        <span class="panel__shader-name">
                                            {{ getShaderDisplayName(shaderPath) }}
                                        </span>
                                        <span
                                            v-if="isShaderUnavailable(shaderPath)"
                                            class="panel__shader-missing"
                                        >
                                            Missing
                                        </span>
                                        <button
                                            class="panel__shader-remove"
                                            type="button"
                                            aria-label="Remove shader"
                                            @click.stop.prevent="
                                                removeShaderFromList(shaderPath)
                                            "
                                        >
                                            <svg
                                                class="panel__shader-remove-icon"
                                                viewBox="0 0 24 24"
                                                fill="none"
                                                stroke="currentColor"
                                                stroke-width="2"
                                                stroke-linecap="round"
                                            >
                                                <path d="M6 6l12 12M18 6l-12 12" />
                                            </svg>
                                        </button>
                                    </div>
                                </div>
                                <div class="panel__shader-list-footer">
                                    <div class="panel__shader-list-footer-left">
                                        <label
                                            v-if="shouldShowMultiShaderToggle"
                                            class="panel__shader-multi-toggle"
                                        >
                                            <span class="panel__shader-multi-label">
                                                Use Multiple Shader
                                            </span>
                                            <span class="panel__toggle panel__toggle--shader-multi">
                                                <input
                                                    class="panel__toggle-input"
                                                    type="checkbox"
                                                    :checked="multiShaderEnabled"
                                                    @change="
                                                        setMultiShaderEnabled(
                                                            ($event.target as HTMLInputElement).checked,
                                                        )
                                                    "
                                                />
                                                <span class="panel__toggle-track">
                                                    <span class="panel__toggle-thumb"></span>
                                                </span>
                                            </span>
                                        </label>
                                    </div>

                                    <div class="panel__shader-list-footer-center">
                                        <button
                                            class="panel__shader-mode-switch"
                                            type="button"
                                            @click="
                                                setRenderingMode(
                                                    isNormalRenderingMode ? 'animeMode' : 'normal',
                                                )
                                            "
                                        >
                                            <span
                                                class="panel__shader-mode-switch-item"
                                                :class="{
                                                    'panel__shader-mode-switch-item--current':
                                                        isNormalRenderingMode,
                                                    'panel__shader-mode-switch-item--enabled':
                                                        isNormalRenderingMode &&
                                                        hasEnabledShaderInCurrentMode,
                                                }"
                                            >
                                                General Mode
                                            </span>
                                            <span
                                                class="panel__shader-mode-switch-item"
                                                :class="{
                                                    'panel__shader-mode-switch-item--current':
                                                        isAnimeModeRenderingMode,
                                                    'panel__shader-mode-switch-item--enabled':
                                                        isAnimeModeRenderingMode &&
                                                        hasEnabledShaderInCurrentMode,
                                                }"
                                            >
                                                Anime Mode
                                            </span>
                                        </button>
                                    </div>

                                    <div class="panel__shader-list-footer-right">
                                        <div
                                            v-if="shouldShowShaderListCollapseToggle"
                                            class="panel__shader-list-actions"
                                        >
                                            <button
                                                class="panel__action panel__action--ghost panel__action--compact panel__shader-toggle"
                                                type="button"
                                                @click="
                                                    isShaderListExpanded = !isShaderListExpanded
                                                "
                                            >
                                                <span>
                                                    {{
                                                        isShaderListExpanded
                                                            ? `Collapse (${selectedShaderFiles.length})`
                                                            : `Show all (${selectedShaderFiles.length})`
                                                    }}
                                                </span>
                                                <svg
                                                    class="panel__shader-toggle-icon"
                                                    viewBox="0 0 20 20"
                                                    fill="none"
                                                    stroke="currentColor"
                                                    stroke-width="2"
                                                    stroke-linecap="round"
                                                    stroke-linejoin="round"
                                                    aria-hidden="true"
                                                >
                                                    <path
                                                        v-if="isShaderListExpanded"
                                                        d="M5 12l5-5 5 5"
                                                    />
                                                    <path
                                                        v-else
                                                        d="M5 8l5 5 5-5"
                                                    />
                                                </svg>
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </template>
            </div>
            <div class="panel__section">
                <div class="panel__subtitle panel__subtitle--large">
                    About
                </div>
                <div class="panel__table panel__table--card">
                    <div class="panel__row panel__row--card" data-window-no-drag>
                        <div class="panel__card-text">
                            <div class="panel__card-title">GitHub</div>
                        </div>
                        <div class="panel__control panel__control--card">
                            <div class="panel__social-actions">
                                <a
                                    class="panel__link-button"
                                    href="https://github.com/FengZeng/soia"
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    data-window-no-drag
                                    @click.prevent="openProjectGithub"
                                >
                                    https://github.com/FengZeng/soia
                                </a>
                            </div>
                        </div>
                    </div>
                    <div class="panel__row panel__row--card" data-window-no-drag>
                        <div class="panel__card-text">
                            <div class="panel__card-title">Reddit</div>
                        </div>
                        <div class="panel__control panel__control--card">
                            <div class="panel__social-actions">
                                <a
                                    class="panel__link-button"
                                    href="https://www.reddit.com/r/soia"
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    data-window-no-drag
                                    @click.prevent="openSubreddit"
                                >
                                    https://www.reddit.com/r/soia
                                </a>
                            </div>
                        </div>
                    </div>
                    <div class="panel__row panel__row--card">
                        <div class="panel__card-text">
                            <div class="panel__card-title">Runtime</div>
                        </div>
                        <div class="panel__control panel__control--card">
                            <span class="panel__value-text">
                                Soia {{ runtimeVersions?.soiaVersion ?? "Unavailable" }}
                                · mpv {{ runtimeVersions?.mpvVersion ?? "Unavailable" }}
                                · FFmpeg {{ runtimeVersions?.ffmpegVersion ?? "Unavailable" }}
                            </span>
                        </div>
                    </div>
                    <div class="panel__row panel__row--card">
                        <div class="panel__card-text">
                            <div class="panel__card-title">Clear All Local Data</div>
                        </div>
                        <div class="panel__control panel__control--card">
                            <button
                                class="panel__reset panel__reset--danger"
                                type="button"
                                :disabled="isFactoryResetInProgress"
                                @click="factoryReset"
                            >
                                {{
                                    isFactoryResetInProgress
                                        ? "Resetting..."
                                        : "Factory Reset"
                                }}
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<style scoped src="../styles/panels.css"></style>
