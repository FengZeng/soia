import { computed, type Ref } from "vue";
import type { HistoryEntry } from "../types/history";
import type { PlaylistEntry } from "../types/playlist";
import type { PlayerApi } from "./usePlaybackController";

type SideActionId = "home" | "history" | "network" | "settings";
type ClearConfirmTarget = "playlist" | "history" | null;

type PlaylistApi = {
    addFromDrawerSelection: (paths: string[]) => Promise<void>;
    clearActivePlaylist: () => Promise<void>;
    removeFromActivePlaylist: (entry: PlaylistEntry) => Promise<void>;
    enterPlaylist: (playlistId: string) => void;
    backToPlaylistList: () => void;
    renamePlaylist: (playlistId: string, name: string) => Promise<void>;
    deletePlaylist: (playlistId: string) => Promise<void>;
    movePlaylist: (fromPlaylistId: string, toPlaylistId: string) => Promise<void>;
    markActivePlaylistAsPlayback: () => Promise<void>;
    toggleLoopOne: (setLoopFile: (enabled: boolean) => Promise<void>) => Promise<void>;
    togglePlaylistLoop: (setLoopFile: (enabled: boolean) => Promise<void>) => Promise<void>;
};

type HistoryApi = {
    clearHistory: () => void;
    removeEntry: (path: string) => void;
    togglePinned: (path: string) => void;
};

type UseAppUiActionsOptions = {
    isMacOS: boolean;
    player: PlayerApi;
    playlistState: PlaylistApi;
    history: HistoryApi;
    historyEntries: Ref<HistoryEntry[]>;
    activePanel: Ref<SideActionId>;
    hideHistory: Ref<boolean>;
    isInfoOpen: Ref<boolean>;
    isPlaylistOpen: Ref<boolean>;
    clearConfirmTarget: Ref<ClearConfirmTarget>;
    playlist: Ref<PlaylistEntry[]>;
    hideAllMenus: () => void;
    schedulePointerRefresh: () => void;
    onStopPlayback: () => Promise<void>;
    playPath: (path: string, preferredTitle?: string) => Promise<void>;
    playPreviousTrack: () => Promise<void>;
    playNextTrack: () => Promise<void>;
};

export const useAppUiActions = ({
    isMacOS,
    player,
    playlistState,
    history,
    historyEntries,
    activePanel,
    hideHistory,
    isInfoOpen,
    isPlaylistOpen,
    clearConfirmTarget,
    playlist,
    hideAllMenus,
    schedulePointerRefresh,
    onStopPlayback,
    playPath,
    playPreviousTrack,
    playNextTrack,
}: UseAppUiActionsOptions) => {
    const isClearConfirmOpen = computed(() => clearConfirmTarget.value !== null);
    const clearConfirmTitle = computed(() => {
        if (clearConfirmTarget.value === "history") return "Clear History";
        return "Clear Playlist";
    });
    const clearConfirmMessage = computed(() => {
        if (clearConfirmTarget.value === "history") return "Clear history?";
        return "Clear the playlist?";
    });

    const onShowPanel = (panel: SideActionId) => {
        activePanel.value = panel;
        hideHistory.value = false;
    };

    const closePlaylist = () => {
        if (!isPlaylistOpen.value) return;
        isPlaylistOpen.value = false;
        schedulePointerRefresh();
    };

    const onNavAction = async (panel: SideActionId) => {
        if (player.state.media.isFileLoaded) {
            closePlaylist();
            await onStopPlayback();
            if (["home", "history", "network"].includes(panel)) {
                player.state.media.url = "";
                player.state.media.title = "";
            }
        }
        onShowPanel(panel);
    };

    const toggleInfo = () => {
        if (!player.state.media.isFileLoaded) return;
        isInfoOpen.value = !isInfoOpen.value;
        if (isInfoOpen.value) {
            isPlaylistOpen.value = false;
        }
    };

    const togglePlaylist = () => {
        isPlaylistOpen.value = !isPlaylistOpen.value;
        if (isPlaylistOpen.value) {
            isInfoOpen.value = false;
        }
        hideAllMenus();
        schedulePointerRefresh();
    };

    const addPlaylistWithFilePicker = async () => {
        const selected = await player.pickFiles();
        if (!selected.length) return;
        await playlistState.addFromDrawerSelection(selected);
    };

    const addPlaylistWithAutoPicker = async () => {
        const selected = await player.pickMediaPathsAuto();
        if (!selected.length) return;
        await playlistState.addFromDrawerSelection(selected);
    };

    const requestAddPlaylistItem = () => {
        if (isMacOS) {
            void addPlaylistWithAutoPicker();
            return;
        }
        void addPlaylistWithFilePicker();
    };

    const onClearHistory = () => {
        if (!historyEntries.value.length) return;
        clearConfirmTarget.value = "history";
    };

    const onRemoveHistory = (entry: HistoryEntry) => {
        history.removeEntry(entry.path);
    };

    const onTogglePinHistory = (entry: HistoryEntry) => {
        history.togglePinned(entry.path);
    };

    const onClearPlaylist = () => {
        if (!playlist.value.length) return;
        clearConfirmTarget.value = "playlist";
    };

    const closeClearConfirm = () => {
        clearConfirmTarget.value = null;
    };

    const onConfirmClear = async () => {
        if (clearConfirmTarget.value === "history") {
            history.clearHistory();
        }
        if (clearConfirmTarget.value === "playlist") {
            await playlistState.clearActivePlaylist();
        }
        clearConfirmTarget.value = null;
    };

    const onRemovePlaylistItem = async (entry: PlaylistEntry) => {
        await playlistState.removeFromActivePlaylist(entry);
    };

    const onPlayPlaylist = async (entry: PlaylistEntry) => {
        await playlistState.markActivePlaylistAsPlayback();
        closePlaylist();
        await playPath(entry.path, entry.title?.trim() || undefined);
    };

    const onEnterPlaylist = (playlistId: string) => {
        playlistState.enterPlaylist(playlistId);
    };

    const onBackToPlaylists = () => {
        playlistState.backToPlaylistList();
    };

    const onRenamePlaylist = async (playlistId: string, name: string) => {
        await playlistState.renamePlaylist(playlistId, name);
    };

    const onDeletePlaylist = async (playlistId: string) => {
        await playlistState.deletePlaylist(playlistId);
    };

    const onMovePlaylist = async (fromPlaylistId: string, toPlaylistId: string) => {
        await playlistState.movePlaylist(fromPlaylistId, toPlaylistId);
    };

    const onPrevTrack = () => playPreviousTrack();

    const onNextTrack = () => playNextTrack();

    const toggleLoopOne = async () => {
        await playlistState.toggleLoopOne(player.setLoopFile);
    };

    const onTogglePlaylistLoop = async () => {
        await playlistState.togglePlaylistLoop(player.setLoopFile);
    };

    return {
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
    };
};
