import type { Ref } from "vue";
import type { ProgressPayload } from "../types/media";
import { AUTO_PLAY_NEXT_IN_PLAYLIST_SETTING_LABEL } from "../mock/settings";
import { loadUiState } from "./useUiStateStore";

type EndFilePayload = {
    reason?: string;
};

type PendingResume = {
    url: string;
    position: number;
};

type PlayerApi = {
    state: {
        media: {
            url: string;
            title: string;
            isFileLoaded: boolean;
            isLivePlayback: boolean;
        };
        playback: {
            duration: number;
        };
        window: {
            isFullscreen: boolean;
        };
    };
    setLoopFile: (enabled: boolean) => Promise<void>;
    syncMpvRenderTarget: () => Promise<void>;
};

type HistoryApi = {
    markPlaybackStarted: (
        path: string,
        position: number,
        duration: number,
        title?: string,
        isLivePlayback?: boolean,
    ) => void;
    recordProgress: (
        path: string,
        position: number,
        duration: number,
        isPlaying: boolean,
        title?: string,
        isLivePlayback?: boolean,
    ) => void;
};

type NowPlayingApi = {
    updateNowPlayingMetadata: () => void;
    captureNowPlayingArtwork: () => Promise<void>;
};

type TracksApi = {
    applyExternalTracksForUrl: (url: string, mediaTitle?: string) => Promise<void>;
};

type UseAppPlaybackEventsOptions = {
    player: PlayerApi;
    tracks: TracksApi;
    history: HistoryApi;
    nowPlaying: NowPlayingApi;
    pendingResume: Ref<PendingResume | null>;
    isLoopOne: Ref<boolean>;
    isLoading: Ref<boolean>;
    loadingUrl: Ref<string>;
    playNextAfterEnd: () => Promise<void>;
};

export const useAppPlaybackEvents = ({
    player,
    tracks,
    history,
    nowPlaying,
    pendingResume,
    isLoopOne,
    isLoading,
    loadingUrl,
    playNextAfterEnd,
}: UseAppPlaybackEventsOptions) => {
    const shouldAutoPlayNextInPlaylist = async () => {
        const stored = await loadUiState<{
            settings?: {
                groups?: Array<{
                    title: string;
                    items: Array<{ label: string; value: string }>;
                }>;
            };
        }>();
        const value = stored?.settings?.groups
            ?.flatMap((group) => group.items)
            .find(
                (item) =>
                    item.label === AUTO_PLAY_NEXT_IN_PLAYLIST_SETTING_LABEL,
            )?.value;
        if (!value) return true;
        return value === "On";
    };

    const handleEndFile = async () => {
        if (!(await shouldAutoPlayNextInPlaylist())) return;
        await playNextAfterEnd();
    };

    const onFileLoaded = () => {
        const pending = pendingResume.value;
        const resumePosition =
            pending && pending.url === player.state.media.url
                ? pending.position
                : 0;
        if (pending && pending.url === player.state.media.url) {
            pendingResume.value = null;
        }

        history.markPlaybackStarted(
            player.state.media.url,
            resumePosition,
            player.state.playback.duration,
            player.state.media.title,
            player.state.media.isLivePlayback,
        );
        void player.setLoopFile(isLoopOne.value);
        isLoading.value = false;
        loadingUrl.value = "";
        nowPlaying.updateNowPlayingMetadata();
        void nowPlaying.captureNowPlayingArtwork();
        void tracks.applyExternalTracksForUrl(
            player.state.media.url,
            player.state.media.title,
        );
        if (player.state.window.isFullscreen) {
            void player.syncMpvRenderTarget();
        }
    };

    const onProgress = (payload: ProgressPayload) => {
        if (!player.state.media.isFileLoaded) return;
        history.recordProgress(
            player.state.media.url,
            payload.time_pos,
            player.state.playback.duration,
            payload.is_playing,
            player.state.media.title,
            player.state.media.isLivePlayback,
        );
    };

    const onEndFile = ({ reason }: EndFilePayload) => {
        if (reason !== "eof") return;
        isLoading.value = false;
        loadingUrl.value = "";
        void handleEndFile();
    };

    const onPlaybackRestart = () => {
        isLoading.value = false;
        loadingUrl.value = "";
    };

    return {
        onFileLoaded,
        onPlaybackRestart,
        onProgress,
        onEndFile,
    };
};
