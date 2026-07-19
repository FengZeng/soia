<script setup lang="ts">
import { computed, ref, watch } from "vue";
import PlayerControls from "./components/PlayerControls.vue";
import PlayerHeader from "./components/PlayerHeader.vue";
import MainPanels from "./components/MainPanels.vue";
import SideActionsNav from "./components/SideActionsNav.vue";
import PlaybackOverlays from "./components/PlaybackOverlays.vue";
import PlaylistPeekButton from "./components/PlaylistPeekButton.vue";
import PlaylistDrawer from "./components/PlaylistDrawer.vue";
import PlaylistCreationDialog from "./components/PlaylistCreationDialog.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import ContextMenu from "./components/ContextMenu.vue";
import OnlineSubtitleDialog from "./components/OnlineSubtitleDialog.vue";
import WindowResizeRegions from "./components/WindowResizeRegions.vue";
import { usePlaybackShortcuts } from "./composables/usePlaybackShortcuts";
import { usePlaybackFlow } from "./composables/usePlaybackFlow";
import { useAppUiPersistence } from "./composables/useAppUiPersistence";
import { useAppRuntimeBindings } from "./composables/useAppRuntimeBindings";
import { useAppPlaybackEvents } from "./composables/useAppPlaybackEvents";
import { useAppUiActions } from "./composables/useAppUiActions";
import { useAppBootstrap } from "./composables/useAppBootstrap";
import { useManualWindowStatePersistence } from "./composables/useManualWindowStatePersistence";
import { usePlaybackOverlays } from "./composables/usePlaybackOverlays";
import { usePlaylistDrawerUi } from "./composables/usePlaylistDrawerUi";
import { useUpdateNotePrompt } from "./composables/useUpdateNotePrompt";
import { useWindowDragRegion } from "./composables/useWindowDragRegion";
import { useMediaInfo } from "./composables/useMediaInfo";
import { usePlaylistEntriesWithProgress } from "./composables/usePlaylistEntriesWithProgress";
import { useAppStartupBindings } from "./composables/useAppStartupBindings";
import { usePlaybackSeekActions } from "./composables/usePlaybackSeekActions";
import { usePlaybackLoadingState } from "./composables/usePlaybackLoadingState";
import { usePlaybackNavigation } from "./composables/usePlaybackNavigation";
import { usePlaybackVolumePersistence } from "./composables/usePlaybackVolumePersistence";
import { usePlaylistCreationPrompt } from "./composables/usePlaylistCreationPrompt";
import { usePlaybackContextMenu } from "./composables/usePlaybackContextMenu";
import { useRemotePlaybackNavigation } from "./composables/useRemotePlaybackNavigation";

const {
    isMacOS,
    player,
    tracks,
    speed,
    adjustments,
    subtitleAppearance,
    history,
    playlistState,
    playlists,
    activePlaylistId,
    activePlaylist,
    playlist,
    loopMode,
    sortMode,
    isLoopOne,
    orderedPlaylist,
    isInfoOpen,
    isPlaylistOpen,
    activePanel,
    clearConfirmTarget,
    isPointerNearLeft,
    isPointerOverUi,
    isPipEnabled,
    schedulePointerRefresh,
    shouldKeepControlsVisible,
    hideAllMenus,
    toggleMenu,
    closeAllMenus,
    isFullscreenTransitioning,
    triggerFullscreenTransition,
    resetFullscreenTransition,
    onToggleFullscreen,
    playerHeaderRef,
    nowPlaying,
    ui,
    isPlaybackActive,
    navActivePanel,
    hasAudioTracks,
    hasSubTracks,
    setWindowControlsVisible,
    normalizeStoredPanel,
} = useAppBootstrap();

const clearNavSelectionDuringLoad = ref(false);
const playbackLoadingState = usePlaybackLoadingState();
const { isLoading, loadingUrl } = playbackLoadingState;
const playlistCreationPrompt = usePlaylistCreationPrompt();
const {
    persistCurrentManualWindow,
    restorePersistedManualWindow,
    schedulePersistManualWindow,
    persistBeforeUnload,
} = useManualWindowStatePersistence({
    isLoading,
    isPlaybackActive,
    isFileLoaded: () => player.state.media.isFileLoaded,
});

const playbackFlow = usePlaybackFlow({
    isMacOS,
    player,
    tracks,
    history,
    playlistState,
    nowPlaying,
    hideAllMenus,
    isInfoOpen,
    loadingState: playbackLoadingState,
    onPlaybackIntent: async () => {
        await persistCurrentManualWindow();
        clearNavSelectionDuringLoad.value = true;
    },
    requestPlaylistCreation: playlistCreationPrompt.requestPlaylistCreation,
    onPlaylistCreated: () => {
        isPlaylistOpen.value = true;
    },
});

const {
    pendingResume,
    hideHistory,
    isLoadingForCurrentUrl,
    playPath,
    onLoadFile,
    onPlayHistory,
    onPlayNetwork,
    onUpdateUrl,
    updateLivePlaybackForDuration,
    resolveMediaTitle,
    onStopPlayback,
    requestOpenFilePicker,
    openSelectedPaths,
    playbackTitleMode,
    compactModeEnabled,
    wallpaperModeEnabled,
    subtitlesDisabled,
} = playbackFlow;

const onStopPlaybackWithWindowRestore = async () => {
    await onStopPlayback();
    await restorePersistedManualWindow();
};

const playbackNavigation = usePlaybackNavigation({
    player,
    playlistState,
    playPath,
});

useRemotePlaybackNavigation({
    playPreviousTrack: playbackNavigation.playPreviousTrack,
    playNextTrack: playbackNavigation.playNextTrack,
});

const isWindowsPlatform =
    typeof navigator !== "undefined" && /\bwindows\b/i.test(navigator.userAgent);
const isLinuxPlatform =
    typeof navigator !== "undefined" && /\blinux\b/i.test(navigator.userAgent);
const playerHeaderCompactModeEnabled = computed(
    () => compactModeEnabled.value || (isWindowsPlatform && isPipEnabled.value),
);
const shouldKeepPlaybackBackgroundOpaque = computed(
    () =>
        (isMacOS && isPipEnabled.value) ||
        (isWindowsPlatform && wallpaperModeEnabled.value),
);
const shouldUseTransparentVideoMode = computed(
    () =>
        player.state.media.isFileLoaded &&
        !shouldKeepPlaybackBackgroundOpaque.value,
);
const sideNavActivePanel = computed(() =>
    isLoading.value && clearNavSelectionDuringLoad.value
        ? null
        : navActivePanel.value,
);
const {
    isLoadingOverlayVisible,
    loadingDownloadSpeedBps,
    seekOverlayLeftText,
    seekOverlayRightText,
    seekOverlayLeftTimelineText,
    volumeOverlayText,
    seekOverlayLeftPulseToken,
    seekOverlayRightPulseToken,
    showSeekOverlay,
    showVolumeOverlay,
} = usePlaybackOverlays({
    player,
    isLoading,
});

const playbackVolume = usePlaybackVolumePersistence(player);

const onSetVolume = async (volume: number) => {
    await playbackVolume.setVolume(volume);
    showVolumeOverlay(player.state.playback.volume);
};

const onToggleMuted = async () => {
    await playbackVolume.toggleMuted();
    showVolumeOverlay(player.state.playback.volume);
};

const {
    isClearConfirmOpen,
    clearConfirmTitle,
    clearConfirmMessage,
    toggleInfo,
    togglePlaylist,
    closePlaylist,
    onNavAction,
    requestAddPlaylistItem,
    onClearHistory,
    onRemoveHistory,
    onTogglePinHistory,
    onClearPlaylist,
    closeClearConfirm,
    onConfirmClear,
    onRemovePlaylistItem,
    onPlayPlaylist,
    onEnterPlaylist,
    onBackToPlaylists,
    onRenamePlaylist,
    onDeletePlaylist,
    onMovePlaylist,
    onPrevTrack,
    onNextTrack,
    toggleLoopOne,
    onTogglePlaylistLoop,
} = useAppUiActions({
    isMacOS,
    player,
    playlistState,
    history,
    historyEntries: history.history,
    activePanel,
    hideHistory,
    isInfoOpen,
    isPlaylistOpen,
    clearConfirmTarget,
    playlist,
    hideAllMenus,
    schedulePointerRefresh,
    onStopPlayback: onStopPlaybackWithWindowRestore,
    playPath,
    playPreviousTrack: playbackNavigation.playPreviousTrack,
    playNextTrack: playbackNavigation.playNextTrack,
});

const onSideNavNavigate = async (
    panel: "home" | "history" | "network" | "settings",
) => {
    clearNavSelectionDuringLoad.value = false;
    await onNavAction(panel);
};

const openSettingsFromPlaybackContextMenu = async () => {
    clearNavSelectionDuringLoad.value = false;
    await onNavAction("settings");
};

const { onSeek, onSeekRelative } = usePlaybackSeekActions({
    player,
    isLoading,
    loadingUrl,
});

const { onKeydown, onDoubleClick } = usePlaybackShortcuts(
    {
        state: player.state,
        togglePlayPause: player.togglePlayPause,
        seekRelative: onSeekRelative,
        setVolume: playbackVolume.setVolume,
    },
    onToggleFullscreen,
    toggleInfo,
    showSeekOverlay,
    showVolumeOverlay,
);

watch(
    subtitlesDisabled,
    (disabled) => {
        void tracks.setSubtitlesDisabled(disabled);
    },
    { immediate: true },
);

const {
    playlistScrollState,
    playlistDrawerWidthRatio,
    onPlaylistScrollPositionChange,
    onPlaylistDrawerWidthRatioChange,
} = usePlaylistDrawerUi();
const {
    isUpdateNotePromptOpen,
    updateNotePromptTitle,
    updateNotePromptBlocks,
    showUpdateNotePrompt,
    closeUpdateNotePrompt,
    onConfirmUpdateNotePrompt,
} = useUpdateNotePrompt({
    activePanel,
    hideHistory,
    clearNavSelectionDuringLoad,
    settingsPanelId: "settings",
});
const {
    onAppMouseDownCapture,
    onAppTouchStartCapture,
    onDragRegionMouseDown,
    onDragRegionTouchStart,
} = useWindowDragRegion();
const { mediaInfo, statusBadges } = useMediaInfo(player);
const currentOrLastPlaybackUrl = computed(
    () => player.state.media.url || player.state.media.lastLoadedUrl,
);
const playlistEntriesWithProgress = usePlaylistEntriesWithProgress(
    orderedPlaylist,
    history.history,
);
const playbackContextMenu = usePlaybackContextMenu({
    isFileLoaded: () => player.state.media.isFileLoaded,
    getCurrentPath: () => player.state.media.url,
    getCurrentTitle: () => player.state.media.title,
    addToFavorites: playlistState.addToFavorites,
    searchOnlineSubtitles: tracks.searchOnlineSubtitleTracks,
    openSubtitleAdvancedSettings: () => {
        tracks.showSubMenu.value = true;
        tracks.showSubtitleAdvancedSettings.value = true;
    },
    openSettings: openSettingsFromPlaybackContextMenu,
    hideAllMenus,
});

const { hasLoadedPanel, loadActivePanel } = useAppUiPersistence({
    activePanel,
    playlists,
    activePlaylistId,
    playlistScrollState,
    playlistDrawerWidthRatio,
    loopMode,
    sortMode,
    playlistState,
    schedulePointerRefresh,
    normalizeStoredPanel,
});

const {
    onFileLoaded: onFileLoadedBase,
    onPlaybackRestart,
    onProgress,
    onEndFile,
} =
    useAppPlaybackEvents({
        player,
        tracks,
        playlistState,
        history,
        nowPlaying,
        pendingResume,
        isLoopOne,
        isLoading,
        loadingUrl,
        playNextAfterEnd: playbackNavigation.playNextAfterEnd,
    });

const onFileLoaded = async () => {
    await onFileLoadedBase();
    if (subtitlesDisabled.value) {
        await tracks.setSubtitlesDisabled(true);
    }
    await adjustments.applyColorAdjustmentsForMedia(player.state.media.url);
    await subtitleAppearance.applySubtitleAppearanceOptions();
};

const onProgressWithLivePlaybackUpdate = (
    payload: Parameters<typeof onProgress>[0],
) => {
    updateLivePlaybackForDuration(payload.duration);
    onProgress(payload);
};

useAppRuntimeBindings({
    player,
    tracks,
    ui,
    onFullscreenTransition: triggerFullscreenTransition,
    onFullscreenTransitionEnd: resetFullscreenTransition,
    onCloseAllMenus: closeAllMenus,
    onKeydown,
    onDoubleClick,
    setWindowControlsVisible,
    onFileLoaded,
    onPlaybackRestart,
    onProgress: onProgressWithLivePlaybackUpdate,
    onEndFile,
    onSourceLoadState: ({ loadingKey, error }) => {
        if (!error) return;
        if (loadingKey && loadingKey !== player.state.media.url) return;
        isLoading.value = false;
        loadingUrl.value = "";
    },
    onPlaybackLoadPrepared: ({ playbackKey, resumePosition }) => {
        player.state.media.url = playbackKey;
        loadingUrl.value = playbackKey;
        pendingResume.value = {
            url: playbackKey,
            position: resumePosition,
        };
    },
    onPlaybackSpeedChange: (playbackSpeed) => {
        speed.currentSpeed.value = playbackSpeed;
    },
    resolveMediaTitle,
    nowPlaying,
    isInfoOpen,
    isPlaylistOpen,
    hideHistory,
    playerHeaderRef,
    closePlaylist,
    shouldKeepControlsVisible,
    schedulePointerRefresh,
});

useAppStartupBindings({
    activePanel,
    hideHistory,
    clearNavSelectionDuringLoad,
    settingsPanelId: "settings",
    isFileLoaded: () => player.state.media.isFileLoaded,
    openSelectedPaths,
    loadActivePanel,
    restorePersistedManualWindow,
    schedulePersistManualWindow,
    persistBeforeUnload,
    showUpdateNotePrompt,
});
</script>

<template>
    <main
        class="soia-container"
        :class="{
            'video-mode': shouldUseTransparentVideoMode,
            'cursor-hidden':
                player.state.media.isFileLoaded &&
                !ui.showControls.value &&
                !isPointerOverUi,
        }"
        @mousedown.capture="onAppMouseDownCapture"
        @touchstart.capture="onAppTouchStartCapture"
        @contextmenu="playbackContextMenu.onContextMenu"
    >
        <SideActionsNav
            v-show="!player.state.media.isFileLoaded || isPointerNearLeft"
            :is-playback-active="isPlaybackActive"
            :active-panel="activePanel"
            :nav-active-panel="sideNavActivePanel"
            @navigate="onSideNavNavigate"
        />

        <PlayerHeader
            ref="playerHeaderRef"
            v-show="ui.showControls.value && !isFullscreenTransitioning"
            :is-mac-os="isMacOS"
            :url="player.state.media.url"
            :media-title="player.state.media.title"
            :is-url-modified="player.isUrlModified.value"
            :is-file-loaded="player.state.media.isFileLoaded"
            :is-info-open="isInfoOpen"
            :is-playlist-open="isPlaylistOpen"
            :is-loading="isLoadingForCurrentUrl"
            :playback-title-mode="playbackTitleMode"
            :compact-mode-enabled="playerHeaderCompactModeEnabled"
            :is-fullscreen="player.state.window.isFullscreen"
            :info="mediaInfo"
            :current-time="player.state.playback.currentTime"
            :duration="player.state.playback.duration"
            :is-playing="player.state.playback.isPlaying"
            :video-bitrate="player.state.playback.videoBitrate"
            :hwdec-current="player.state.playback.hwdecCurrent"
            :playback-speed="speed.currentSpeed.value"
            :video-tracks="tracks.videoTracks.value"
            :audio-tracks="tracks.audioTracks.value"
            :sub-tracks="tracks.subTracks.value"
            @update:url="onUpdateUrl"
            @load-file="onLoadFile"
            @open-file-picker="requestOpenFilePicker"
            @toggle-info="toggleInfo"
            @toggle-playlist="togglePlaylist"
            @url-input-mousedown="onDragRegionMouseDown"
            @url-input-touchstart="onDragRegionTouchStart"
        />

        <PlaybackOverlays
            :is-loading="isLoadingOverlayVisible"
            :loading-speed-bps="loadingDownloadSpeedBps"
            :show-status-overlay="
                player.state.media.isFileLoaded &&
                nowPlaying.showStatusOverlay.value
            "
            :status-overlay-mode="nowPlaying.statusOverlayMode.value"
            :seek-overlay-left-text="seekOverlayLeftText"
            :seek-overlay-right-text="seekOverlayRightText"
            :seek-overlay-left-timeline-text="seekOverlayLeftTimelineText"
            :volume-overlay-text="volumeOverlayText"
            :hide-seek-timeline="ui.showControls.value"
            :seek-overlay-left-pulse-token="seekOverlayLeftPulseToken"
            :seek-overlay-right-pulse-token="seekOverlayRightPulseToken"
        />

        <MainPanels
            v-show="!player.state.media.isFileLoaded"
            :is-file-loaded="player.state.media.isFileLoaded"
            :hover="ui.hoverFilePicker.value"
            :history="history.history.value"
            :history-ready="history.isReady.value"
            :hide-history="hideHistory"
            :mode="activePanel"
            :current-url="currentOrLastPlaybackUrl"
            @update:hover="ui.hoverFilePicker.value = $event"
            @open-file-picker="requestOpenFilePicker"
            @play-history="onPlayHistory"
            @play-network="onPlayNetwork"
            @clear-history="onClearHistory"
            @remove-history="onRemoveHistory"
            @toggle-pin-history="onTogglePinHistory"
        />

        <PlaylistDrawer
            :open="isPlaylistOpen"
            :playlists="playlists"
            :active-playlist-id="activePlaylistId"
            :active-playlist-name="activePlaylist?.name ?? ''"
            :width-ratio="playlistDrawerWidthRatio"
            :scroll-state="playlistScrollState"
            :entries="playlistEntriesWithProgress"
            :is-ready="hasLoadedPanel"
            :current-url="player.state.media.url"
            :loop-mode="loopMode"
            :sort-mode="sortMode"
            :is-loop-one="isLoopOne"
            @close="closePlaylist"
            @add="requestAddPlaylistItem"
            @toggle-loop="onTogglePlaylistLoop"
            @toggle-sort="playlistState.cycleSortMode"
            @clear="onClearPlaylist"
            @remove="onRemovePlaylistItem"
            @play="onPlayPlaylist"
            @enter-playlist="onEnterPlaylist"
            @back="onBackToPlaylists"
            @rename-playlist="onRenamePlaylist"
            @delete-playlist="onDeletePlaylist"
            @move-playlist="onMovePlaylist"
            @width-ratio-change="onPlaylistDrawerWidthRatioChange"
            @scroll-position-change="onPlaylistScrollPositionChange"
        />

        <ContextMenu
            :open="playbackContextMenu.isOpen.value"
            :x="playbackContextMenu.position.value.x"
            :y="playbackContextMenu.position.value.y"
            :items="playbackContextMenu.items.value"
            @select="playbackContextMenu.onSelect"
            @close="playbackContextMenu.close"
        />

        <OnlineSubtitleDialog
            :open="tracks.isOnlineSubtitleDialogOpen.value"
            :provider-tabs="tracks.onlineSubtitleProviderTabs.value"
            :active-provider-id="tracks.activeOnlineSubtitleProviderId.value"
            :results="tracks.onlineSubtitleResults.value"
            :loading="tracks.isSearchingOnlineSubtitles.value"
            :applying="tracks.isLoadingOnlineSubtitle.value"
            :error-message="tracks.onlineSubtitleErrorMessage.value"
            @close="tracks.closeOnlineSubtitleDialog"
            @provider-change="tracks.setOnlineSubtitleProvider"
            @select="tracks.addSelectedOnlineSubtitleTrack"
        />

        <PlayerControls
            :is-playing="player.state.playback.isPlaying"
            :current-time="player.state.playback.currentTime"
            :duration="player.state.playback.duration"
            :is-live-playback="player.state.media.isLivePlayback"
            :volume="player.state.playback.volume"
            :progress-percent="player.progressPercent.value"
            :buffered-percent="player.bufferedPercent.value"
            :format-time="player.formatTime"
            :controls-visible="ui.showControls.value"
            :is-hidden="
                !player.state.media.isFileLoaded || !ui.showControls.value
            "
            :status-badges="statusBadges"
            :current-speed="speed.currentSpeed.value"
            :playback-rates="speed.playbackRates"
            :show-speed-menu="speed.showSpeedMenu.value"
            :show-settings-menu="adjustments.showSettingsMenu.value"
            :audio-delay="adjustments.audioDelay.value"
            :sub-delay="adjustments.subDelay.value"
            :secondary-sub-delay="adjustments.secondarySubDelay.value"
            :brightness="adjustments.brightness.value"
            :contrast="adjustments.contrast.value"
            :saturation="adjustments.saturation.value"
            :gamma="adjustments.gamma.value"
            :hue="adjustments.hue.value"
            :global-color-adjustments-enabled="
                adjustments.globalColorAdjustmentsEnabled.value
            "
            :is-loop-one="isLoopOne"
            :audio-tracks="tracks.audioTracks.value"
            :show-audio-menu="tracks.showAudioMenu.value"
            :sub-tracks="tracks.subTracks.value"
            :dual-sub-enabled="tracks.dualSubEnabled.value"
            :secondary-sub-id="tracks.secondarySubId.value"
            :active-sub-target="tracks.activeSubTarget.value"
            :primary-sub-font-family="subtitleAppearance.primaryFontFamily.value"
            :secondary-sub-font-family="subtitleAppearance.secondaryFontFamily.value"
            :primary-sub-font-size="subtitleAppearance.primaryFontSize.value"
            :secondary-sub-font-size="subtitleAppearance.secondaryFontSize.value"
            :primary-sub-font-color="subtitleAppearance.primaryFontColor.value"
            :secondary-sub-font-color="subtitleAppearance.secondaryFontColor.value"
            :primary-sub-pos="subtitleAppearance.primarySubPos.value"
            :secondary-sub-pos="subtitleAppearance.secondarySubPos.value"
            :show-sub-menu="tracks.showSubMenu.value"
            :show-subtitle-advanced-settings="tracks.showSubtitleAdvancedSettings.value"
            :has-audio-tracks="hasAudioTracks"
            :has-sub-tracks="hasSubTracks"
            :is-fullscreen="player.state.window.isFullscreen"
            @seek="onSeek"
            @prev-track="onPrevTrack"
            @toggle-play-pause="player.togglePlayPause"
            @stop-playback="onStopPlaybackWithWindowRestore"
            @next-track="onNextTrack"
            @toggle-menu="toggleMenu"
            @toggle-loop-one="toggleLoopOne"
            @set-speed="speed.setSpeed"
            @set-volume="onSetVolume"
            @toggle-muted="onToggleMuted"
            @set-audio-delay="adjustments.setAudioDelay"
            @set-sub-delay-for-target="adjustments.setSubDelayForTarget"
            @set-sub-font-family="subtitleAppearance.setSubtitleFontFamily"
            @set-sub-font-size="subtitleAppearance.setSubtitleFontSize"
            @set-sub-font-color="subtitleAppearance.setSubtitleFontColor"
            @set-sub-position="subtitleAppearance.setSubtitlePosition"
            @reset-sub-appearance="subtitleAppearance.resetSubtitleAppearance"
            @set-brightness="adjustments.setBrightness"
            @set-contrast="adjustments.setContrast"
            @set-saturation="adjustments.setSaturation"
            @set-gamma="adjustments.setGamma"
            @set-hue="adjustments.setHue"
            @set-global-color-adjustments-enabled="
                adjustments.setGlobalColorAdjustmentsEnabled
            "
            @select-audio="tracks.selectAudio"
            @select-sub-track="tracks.selectSubTrack"
            @set-active-sub-target="tracks.setActiveSubTarget"
            @toggle-dual-sub="tracks.setDualSubEnabled"
            @add-external-audio="tracks.addExternalAudioTrack"
            @add-external-sub="tracks.addExternalSubtitleTrack"
            @find-online-sub="player.state.media.url.trim() && tracks.searchOnlineSubtitleTracks(player.state.media.url, player.state.media.title || undefined)"
            @toggle-fullscreen="onToggleFullscreen"
            @update:show-subtitle-advanced-settings="tracks.showSubtitleAdvancedSettings.value = $event"
        />

        <PlaylistPeekButton
            v-show="!isPlaylistOpen"
            :disabled="
                tracks.showAudioMenu.value ||
                tracks.showSubMenu.value ||
                speed.showSpeedMenu.value ||
                adjustments.showSettingsMenu.value
            "
            @toggle="togglePlaylist"
        />

        <ConfirmDialog
            :open="isClearConfirmOpen"
            :title="clearConfirmTitle"
            :message="clearConfirmMessage"
            confirm-text="Clear"
            @cancel="closeClearConfirm"
            @confirm="onConfirmClear"
        />

        <ConfirmDialog
            :open="isUpdateNotePromptOpen"
            :title="updateNotePromptTitle"
            message=""
            confirm-text="Update"
            cancel-text="Cancel"
            confirm-variant="primary"
            size="wide"
            @cancel="closeUpdateNotePrompt"
            @confirm="onConfirmUpdateNotePrompt"
        >
            <div class="update-note">
                <template v-if="updateNotePromptBlocks.length">
                    <template
                        v-for="(block, blockIndex) in updateNotePromptBlocks"
                        :key="blockIndex"
                    >
                        <div
                            v-if="block.type === 'heading'"
                            class="update-note__heading"
                            :class="{
                                'update-note__heading--section': block.level <= 3,
                            }"
                        >
                            {{ block.text }}
                        </div>
                        <p
                            v-else-if="block.type === 'paragraph'"
                            class="update-note__paragraph"
                        >
                            {{ block.text }}
                        </p>
                        <ol
                            v-else-if="block.ordered"
                            class="update-note__list update-note__list--ordered"
                        >
                            <li
                                v-for="(item, itemIndex) in block.items"
                                :key="itemIndex"
                            >
                                {{ item }}
                            </li>
                        </ol>
                        <ul v-else class="update-note__list">
                            <li
                                v-for="(item, itemIndex) in block.items"
                                :key="itemIndex"
                            >
                                {{ item }}
                            </li>
                        </ul>
                    </template>
                </template>
                <p v-else class="update-note__paragraph">
                    A new version is ready to install.
                </p>
            </div>
        </ConfirmDialog>

        <PlaylistCreationDialog
            :open="playlistCreationPrompt.isOpen.value"
            :message="playlistCreationPrompt.message.value"
            v-model:name-draft="playlistCreationPrompt.nameDraft.value"
            @cancel="playlistCreationPrompt.cancelPlaylistCreation"
            @confirm="playlistCreationPrompt.confirmPlaylistCreation"
        />

        <WindowResizeRegions
            v-if="isLinuxPlatform && !player.state.window.isFullscreen"
        />
    </main>
</template>

<style src="./styles/app-theme.css"></style>
<style scoped src="./styles/app-shell.css"></style>
<style src="./styles/player.css"></style>
<style scoped>
.update-note {
    display: grid;
    gap: 10px;
    max-height: min(46vh, 360px);
    overflow: auto;
    padding-right: 2px;
}

.update-note__heading {
    margin-top: 2px;
    color: rgba(255, 255, 255, 0.74);
    font-size: 12px;
    font-weight: 650;
}

.update-note__heading--section {
    color: rgba(255, 255, 255, 0.92);
    font-size: 13px;
}

.update-note__paragraph {
    margin: 0;
}

.update-note__list {
    margin: 0;
    padding-left: 18px;
}

.update-note__list li + li {
    margin-top: 6px;
}

:global(:root[data-theme="light"]) .update-note__heading {
    color: rgba(33, 45, 60, 0.72);
}

:global(:root[data-theme="light"]) .update-note__heading--section {
    color: rgba(33, 45, 60, 0.92);
}

:global(:root[data-theme="graphite"]) .update-note__heading {
    color: rgba(220, 226, 234, 0.78);
}

:global(:root[data-theme="graphite"]) .update-note__heading--section {
    color: rgba(237, 241, 246, 0.95);
}
</style>
