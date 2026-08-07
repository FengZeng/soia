import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { MediaTrack } from "../types/media";
import { normalizeLocalPathForCompare } from "../utils/localMediaSiblings";
import {
    OPENSUBTITLES_ENABLED_SETTING_LABEL,
    SUBSOURCE_ENABLED_SETTING_LABEL,
} from "../mock/settings";
import { loadUiState } from "./useUiStateStore";
import { useSubtitleState, type SubtitleTarget } from "./useSubtitleState";
import type { CoreClient } from "../core-client/CoreClient";
import type { MediaTrackDto } from "../core-client/generated/MediaTrackDto";

type HistoryApi = {
    getExternalTracks: (path: string) => { audio: string[]; sub: string[] };
    recordExternalTrack: (
        path: string,
        kind: "audio" | "sub",
        trackPath: string,
    ) => Promise<void>;
};

type ExternalSubtitleMatch = {
    path: string;
    name?: string;
};

type OnlineSubtitleDownload = {
    path: string;
    title: string;
};

export type OnlineSubtitleSearchResult = {
    id: string;
    providerId: string;
    downloadId: string;
    fileId?: number;
    title: string;
    fileName: string;
    language: string;
    downloads?: number | null;
};

export type OnlineSubtitleProviderId = "opensubtitles" | "subsource";

export type OnlineSubtitleProviderTab = {
    id: OnlineSubtitleProviderId;
    label: string;
    count: number;
    loading: boolean;
    searched: boolean;
};

type BackgroundSubtitleItem = {
    path: string;
    title?: string;
    mode: "select" | "auto";
    mediaKey: string;
};

type TrackUpdateWaiter = {
    resolve: () => void;
    timeoutId: ReturnType<typeof setTimeout>;
};

type StoredSettingsState = {
    settings?: {
        groups?: Array<{
            items?: Array<{
                label?: string;
                value?: string;
            }>;
        }>;
    };
};

const TRACK_UPDATE_WAIT_TIMEOUT_MS = 700;
const BACKGROUND_TRACK_ADD_GAP_MS = 40;
const VISIBLE_MENU_TRACK_ADD_GAP_MS = 180;
const DEFAULT_ONLINE_SUBTITLE_PROVIDER_ID: OnlineSubtitleProviderId = "opensubtitles";
const onlineSubtitleProviderOptions: Array<{
    id: OnlineSubtitleProviderId;
    label: string;
    enabledSettingLabel: string;
}> = [
    {
        id: "opensubtitles",
        label: "OpenSubtitles",
        enabledSettingLabel: OPENSUBTITLES_ENABLED_SETTING_LABEL,
    },
    {
        id: "subsource",
        label: "SubSource",
        enabledSettingLabel: SUBSOURCE_ENABLED_SETTING_LABEL,
    },
];

const isSettingEnabled = (value?: string) => value === "On";

const getStoredSettingValue = (
    state: StoredSettingsState | null,
    label: string,
): string | undefined => {
    for (const group of state?.settings?.groups ?? []) {
        const item = group.items?.find((candidate) => candidate.label === label);
        if (item?.value) return item.value;
    }
    return undefined;
};

const resolveInitialOnlineSubtitleProvider = async (): Promise<OnlineSubtitleProviderId> => {
    const stored = await loadUiState<StoredSettingsState>();
    return (
        onlineSubtitleProviderOptions.find((provider) =>
            isSettingEnabled(getStoredSettingValue(stored, provider.enabledSettingLabel)),
        )?.id ?? DEFAULT_ONLINE_SUBTITLE_PROVIDER_ID
    );
};

const subtitleExtensions = [
    "srt",
    "ass",
    "ssa",
    "vtt",
    "sub",
    "idx",
    "sup",
    "smi",
    "smil",
    "lrc",
    "ttml",
    "dfxp",
];

export const useMediaTracks = (
    coreClient: CoreClient,
    getCurrentMediaUrl?: () => string,
    history?: HistoryApi,
) => {
    const videoTracks = ref<MediaTrack[]>([]);
    const audioTracks = ref<MediaTrack[]>([]);
    const subTracks = ref<MediaTrack[]>([]);
    const activeOnlineSubtitleProviderId = ref<OnlineSubtitleProviderId>(
        DEFAULT_ONLINE_SUBTITLE_PROVIDER_ID,
    );
    const onlineSubtitleResultMap = ref<Record<OnlineSubtitleProviderId, OnlineSubtitleSearchResult[]>>({
        opensubtitles: [],
        subsource: [],
    });
    const onlineSubtitleErrorMap = ref<Record<OnlineSubtitleProviderId, string>>({
        opensubtitles: "",
        subsource: "",
    });
    const onlineSubtitleLoadingMap = ref<Record<OnlineSubtitleProviderId, boolean>>({
        opensubtitles: false,
        subsource: false,
    });
    const onlineSubtitleSearchedMap = ref<Record<OnlineSubtitleProviderId, boolean>>({
        opensubtitles: false,
        subsource: false,
    });
    const onlineSubtitleProviderTabs = computed<OnlineSubtitleProviderTab[]>(() =>
        onlineSubtitleProviderOptions.map((provider) => ({
            id: provider.id,
            label: provider.label,
            count: onlineSubtitleResultMap.value[provider.id].length,
            loading: onlineSubtitleLoadingMap.value[provider.id],
            searched: onlineSubtitleSearchedMap.value[provider.id],
        })),
    );
    const onlineSubtitleResults = computed(
        () => onlineSubtitleResultMap.value[activeOnlineSubtitleProviderId.value],
    );
    const onlineSubtitleErrorMessage = computed(
        () => onlineSubtitleErrorMap.value[activeOnlineSubtitleProviderId.value],
    );
    const isSearchingOnlineSubtitles = computed(
        () => onlineSubtitleLoadingMap.value[activeOnlineSubtitleProviderId.value],
    );
    const isOnlineSubtitleDialogOpen = ref(false);
    const isLoadingOnlineSubtitle = ref(false);

    const showAudioMenu = ref(false);
    const showSubMenu = ref(false);
    const showSubtitleAdvancedSettings = ref(false);
    const subtitleState = useSubtitleState(subTracks, coreClient);
    let pendingTracksUpdate: { tracks: MediaTrack[] } | null = null;
    let tracksUpdateFrame: number | null = null;
    let backgroundSubtitleQueue: BackgroundSubtitleItem[] = [];
    let isAddingBackgroundSubtitle = false;
    let onlineSubtitleMediaKey = "";
    let onlineSubtitleMediaTitle: string | undefined;
    let onlineSubtitleSearchGeneration = 0;
    let backgroundSubtitleGeneration = 0;
    let trackUpdateWaiters: TrackUpdateWaiter[] = [];

    const normalizeSelectedPaths = (
        selected: string | string[] | null,
    ): string[] => {
        if (!selected) return [];
        return Array.isArray(selected) ? selected : [selected];
    };
    const isSameTrackId = (left: MediaTrack["id"], right: MediaTrack["id"]) =>
        String(left) === String(right);

    const getMediaKey = (explicitUrl?: string): string | null => {
        const url = (explicitUrl ?? getCurrentMediaUrl?.() ?? "").trim();
        if (!url) return null;
        return url;
    };

    const recordExternalTrack = async (kind: "audio" | "sub", path: string) => {
        const mediaKey = getMediaKey();
        if (!mediaKey || !history) return;
        await history.recordExternalTrack(mediaKey, kind, path);
    };

    const titleFromPath = (path: string): string => {
        const normalized = path.replace(/\\/g, "/");
        return normalized.split("/").filter(Boolean).pop() || path;
    };

    const getAutoSubtitleMatches = async (
        mediaKey: string,
        mediaTitle?: string,
    ): Promise<ExternalSubtitleMatch[]> => {
        const matches = await invoke<ExternalSubtitleMatch[]>(
            "find_fuzzy_external_subtitle_matches",
            {
                payload: {
                    playbackKey: mediaKey,
                    mediaTitle,
                },
            },
        ).catch(() => []);
        return matches.filter((match) => !!match.path);
    };

    const addExternalSubPath = async (
        path: string,
        mode: "select" | "auto",
        mediaKey?: string,
        title?: string,
    ): Promise<boolean> => {
        if (mediaKey && getMediaKey() !== mediaKey) return false;
        try {
            const displayTitle = title?.trim() || titleFromPath(path);
            await invoke("mpv_run_command", {
                args: ["sub-add", path, mode, displayTitle],
            });
            return true;
        } catch {
            // ignore if file is missing or mpv rejects it
            return false;
        }
    };

    const waitForBackgroundTrackGap = () =>
        new Promise<void>((resolve) => {
            const gapMs =
                showAudioMenu.value || showSubMenu.value
                    ? VISIBLE_MENU_TRACK_ADD_GAP_MS
                    : BACKGROUND_TRACK_ADD_GAP_MS;
            setTimeout(resolve, gapMs);
        });

    const waitForNextTracksUpdate = (timeoutMs = TRACK_UPDATE_WAIT_TIMEOUT_MS) => {
        let waiter: TrackUpdateWaiter | null = null;
        const promise = new Promise<void>((resolve) => {
            const finish = () => {
                if (!waiter) return;
                trackUpdateWaiters = trackUpdateWaiters.filter((item) => item !== waiter);
                clearTimeout(waiter.timeoutId);
                waiter = null;
                resolve();
            };
            waiter = {
                resolve: finish,
                timeoutId: setTimeout(finish, timeoutMs),
            };
            trackUpdateWaiters.push(waiter);
        });
        return {
            promise,
            cancel: () => {
                if (!waiter) return;
                trackUpdateWaiters = trackUpdateWaiters.filter((item) => item !== waiter);
                clearTimeout(waiter.timeoutId);
                waiter = null;
            },
        };
    };

    const notifyTrackUpdateWaiters = () => {
        const waiters = trackUpdateWaiters;
        trackUpdateWaiters = [];
        waiters.forEach((waiter) => waiter.resolve());
    };

    const clearBackgroundSubtitleQueue = () => {
        backgroundSubtitleGeneration += 1;
        backgroundSubtitleQueue = [];
        notifyTrackUpdateWaiters();
    };

    const runNextBackgroundSubtitle = async () => {
        if (isAddingBackgroundSubtitle) return;
        if (!backgroundSubtitleQueue.length) return;
        const generation = backgroundSubtitleGeneration;
        const next = backgroundSubtitleQueue.shift();
        if (!next) return;
        if (getMediaKey() !== next.mediaKey) {
            clearBackgroundSubtitleQueue();
            return;
        }
        isAddingBackgroundSubtitle = true;
        const trackUpdateWaiter = waitForNextTracksUpdate();
        const added = await addExternalSubPath(
            next.path,
            next.mode,
            next.mediaKey,
            next.title,
        );
        if (generation !== backgroundSubtitleGeneration) {
            trackUpdateWaiter.cancel();
            isAddingBackgroundSubtitle = false;
            void runNextBackgroundSubtitle();
            return;
        }
        if (added) {
            await trackUpdateWaiter.promise;
        } else {
            trackUpdateWaiter.cancel();
        }
        isAddingBackgroundSubtitle = false;
        if (generation !== backgroundSubtitleGeneration) {
            void runNextBackgroundSubtitle();
            return;
        }
        await waitForBackgroundTrackGap();
        void runNextBackgroundSubtitle();
    };

    const enqueueBackgroundSubtitles = (items: BackgroundSubtitleItem[]) => {
        if (!items.length) return;
        backgroundSubtitleQueue.push(...items);
        void runNextBackgroundSubtitle();
    };

    const processTracksUpdate = (payload: { tracks: MediaTrack[] }) => {
        const all = payload.tracks;
        videoTracks.value = all.filter((t) => t.track_type === "video");
        audioTracks.value = all.filter((t) => t.track_type === "audio");
        const subs = all.filter((t) => t.track_type === "sub");
        const previousPrimaryId = subtitleState.primarySubId.value;
        const selectedIds = subs
            .filter((track) => track.selected)
            .map((track) => track.id);
        const hasSelected = (id: MediaTrack["id"]) =>
            selectedIds.some((selectedId) => isSameTrackId(selectedId, id));
        const normalizedPrimaryId = (() => {
            if (!selectedIds.length) return 0;
            if (selectedIds.length === 1) return selectedIds[0];
            if (previousPrimaryId !== 0 && hasSelected(previousPrimaryId)) {
                return previousPrimaryId;
            }
            const nonSecondary = selectedIds.find(
                (id) => !isSameTrackId(id, subtitleState.secondarySubId.value),
            );
            return nonSecondary ?? selectedIds[0];
        })();
        const normalizedSubs = subs.map((track) => ({
            ...track,
            selected:
                normalizedPrimaryId !== 0 &&
                isSameTrackId(track.id, normalizedPrimaryId),
        }));
        void subtitleState.reconcileSubtitleState(normalizedSubs);
        subTracks.value = [
            {
                id: 0,
                title: "None (Off)",
                lang: "",
                selected: normalizedPrimaryId === 0,
                track_type: "sub",
            },
            ...normalizedSubs,
        ];
        notifyTrackUpdateWaiters();
    };

    const mediaTrackFromDto = (track: MediaTrackDto): MediaTrack => ({
        id: track.id,
        track_type: track.trackType,
        title: track.title,
        lang: track.lang,
        selected: track.selected,
        codec: track.codec ?? undefined,
        codec_desc: track.codecDesc ?? undefined,
        decoder_desc: track.decoderDesc ?? undefined,
        demux_w: track.demuxW ?? undefined,
        demux_h: track.demuxH ?? undefined,
        demux_fps: track.demuxFps ?? undefined,
        demux_bitrate: track.demuxBitrate ?? undefined,
        demux_samplerate: track.demuxSamplerate ?? undefined,
        demux_channels: track.demuxChannels ?? undefined,
        demux_channel_count: track.demuxChannelCount ?? undefined,
        fps: track.fps ?? undefined,
        w: track.w ?? undefined,
        h: track.h ?? undefined,
        is_default: track.isDefault ?? undefined,
        forced: track.forced ?? undefined,
        external: track.external ?? undefined,
    });

    const handleTracksSnapshot = (tracks: MediaTrackDto[]) => {
        handleTracksUpdate({ tracks: tracks.map(mediaTrackFromDto) });
    };

    const handleTracksUpdate = (payload: { tracks: MediaTrack[] }) => {
        pendingTracksUpdate = payload;
        if (tracksUpdateFrame != null) return;
        const flushTracksUpdate = () => {
            tracksUpdateFrame = null;
            const latest = pendingTracksUpdate;
            pendingTracksUpdate = null;
            if (latest) {
                processTracksUpdate(latest);
            }
        };
        if (typeof window === "undefined") {
            setTimeout(flushTracksUpdate, 0);
            return;
        }
        tracksUpdateFrame = window.requestAnimationFrame(flushTracksUpdate);
    };

    const selectAudio = async (track: MediaTrack) => {
        await coreClient.execute({
            type: "selectAudioTrack",
            trackId: Number(track.id),
        });
    };

    const selectSubTrack = async (payload: {
        target: SubtitleTarget;
        track: MediaTrack;
    }) => {
        await subtitleState.selectSubTrack(payload);
    };

    const setDualSubEnabled = async (enabled: boolean) => {
        await subtitleState.setDualSubEnabled(enabled);
    };

    const addExternalAudioTrack = async () => {
        const selected = await open({
            multiple: false,
            directory: false,
        });
        const [path] = normalizeSelectedPaths(selected);
        if (!path) return;
        await recordExternalTrack("audio", path);
        await invoke("mpv_run_command", { args: ["audio-add", path, "select"] });
    };

    const addExternalSubtitleTrack = async () => {
        const selected = await open({
            multiple: false,
            directory: false,
            filters: [
                {
                    name: "Subtitle Files",
                    extensions: subtitleExtensions,
                },
            ],
        });
        const [path] = normalizeSelectedPaths(selected);
        if (!path) return;
        await recordExternalTrack("sub", path);
        await invoke("mpv_run_command", {
            args: ["sub-add", path, "select", titleFromPath(path)],
        });
    };

    const searchOnlineSubtitleTracks = async (
        playbackKey: string,
        mediaTitle?: string,
    ): Promise<void> => {
        const mediaKey = getMediaKey(playbackKey);
        if (!mediaKey) return;
        onlineSubtitleSearchGeneration += 1;
        const generation = onlineSubtitleSearchGeneration;
        onlineSubtitleMediaKey = mediaKey;
        onlineSubtitleMediaTitle = mediaTitle;
        const initialProviderId = await resolveInitialOnlineSubtitleProvider();
        if (
            getMediaKey() !== mediaKey ||
            generation !== onlineSubtitleSearchGeneration
        ) return;
        activeOnlineSubtitleProviderId.value = initialProviderId;
        onlineSubtitleResultMap.value = {
            opensubtitles: [],
            subsource: [],
        };
        onlineSubtitleErrorMap.value = {
            opensubtitles: "",
            subsource: "",
        };
        onlineSubtitleLoadingMap.value = {
            opensubtitles: false,
            subsource: false,
        };
        onlineSubtitleSearchedMap.value = {
            opensubtitles: false,
            subsource: false,
        };
        isOnlineSubtitleDialogOpen.value = true;
        await searchOnlineSubtitleProvider(
            initialProviderId,
            mediaKey,
            mediaTitle,
            generation,
        );
    };

    const searchOnlineSubtitleProvider = async (
        providerId: OnlineSubtitleProviderId,
        mediaKey: string,
        mediaTitle: string | undefined,
        generation: number,
    ): Promise<void> => {
        onlineSubtitleSearchedMap.value = {
            ...onlineSubtitleSearchedMap.value,
            [providerId]: true,
        };
        onlineSubtitleLoadingMap.value = {
            ...onlineSubtitleLoadingMap.value,
            [providerId]: true,
        };
        onlineSubtitleErrorMap.value = {
            ...onlineSubtitleErrorMap.value,
            [providerId]: "",
        };
        try {
            const results = await invoke<OnlineSubtitleSearchResult[]>(
                "search_online_subtitles",
                {
                    payload: {
                        playbackKey: mediaKey,
                        mediaTitle,
                        providerId,
                    },
                },
            );
            if (
                getMediaKey() !== mediaKey ||
                generation !== onlineSubtitleSearchGeneration
            ) return;
            onlineSubtitleResultMap.value = {
                ...onlineSubtitleResultMap.value,
                [providerId]: results,
            };
        } catch (error) {
            if (
                getMediaKey() !== mediaKey ||
                generation !== onlineSubtitleSearchGeneration
            ) return;
            onlineSubtitleErrorMap.value = {
                ...onlineSubtitleErrorMap.value,
                [providerId]: String(error),
            };
        } finally {
            if (
                getMediaKey() === mediaKey &&
                generation === onlineSubtitleSearchGeneration
            ) {
                onlineSubtitleLoadingMap.value = {
                    ...onlineSubtitleLoadingMap.value,
                    [providerId]: false,
                };
            }
        }
    };

    const setOnlineSubtitleProvider = (providerId: OnlineSubtitleProviderId) => {
        if (activeOnlineSubtitleProviderId.value === providerId) return;
        activeOnlineSubtitleProviderId.value = providerId;
        const mediaKey = onlineSubtitleMediaKey;
        if (!mediaKey) return;
        if (onlineSubtitleSearchedMap.value[providerId]) return;
        void searchOnlineSubtitleProvider(
            providerId,
            mediaKey,
            onlineSubtitleMediaTitle,
            onlineSubtitleSearchGeneration,
        );
    };

    const closeOnlineSubtitleDialog = () => {
        if (isLoadingOnlineSubtitle.value) return;
        isOnlineSubtitleDialogOpen.value = false;
    };

    const addSelectedOnlineSubtitleTrack = async (
        result: OnlineSubtitleSearchResult,
    ): Promise<boolean> => {
        const mediaKey = onlineSubtitleMediaKey;
        if (!mediaKey || getMediaKey() !== mediaKey) return false;
        isLoadingOnlineSubtitle.value = true;
        onlineSubtitleErrorMap.value = {
            ...onlineSubtitleErrorMap.value,
            [result.providerId as OnlineSubtitleProviderId]: "",
        };
        try {
            const subtitle = await invoke<OnlineSubtitleDownload>(
                "download_online_subtitle",
                {
                    payload: {
                        providerId: result.providerId,
                        downloadId: result.downloadId,
                        fileId: result.fileId,
                        fileName: result.fileName,
                        title: result.title || onlineSubtitleMediaTitle || result.fileName,
                    },
                },
            );
            if (getMediaKey() !== mediaKey) return false;
            const added = await addExternalSubPath(
                subtitle.path,
                "select",
                mediaKey,
                subtitle.title,
            );
            if (added) {
                await recordExternalTrack("sub", subtitle.path);
                isOnlineSubtitleDialogOpen.value = false;
            }
            return added;
        } catch (error) {
            onlineSubtitleErrorMap.value = {
                ...onlineSubtitleErrorMap.value,
                [result.providerId as OnlineSubtitleProviderId]: String(error),
            };
            return false;
        } finally {
            isLoadingOnlineSubtitle.value = false;
        }
    };

    const applyExternalTracksForUrl = async (url: string, mediaTitle?: string) => {
        const mediaKey = getMediaKey(url);
        if (!mediaKey) return;
        clearBackgroundSubtitleQueue();
        const { audio: audioPaths, sub: subPaths } = history
            ? history.getExternalTracks(mediaKey)
            : { audio: [], sub: [] };
        const rememberedSubPaths = new Set(subPaths.map(normalizeLocalPathForCompare));
        if (getMediaKey() !== mediaKey) return;
        for (const audioPath of audioPaths) {
            if (getMediaKey() !== mediaKey) return;
            try {
                await invoke("mpv_run_command", {
                    args: ["audio-add", audioPath, "select"],
                });
            } catch {
                // ignore if file is missing or mpv rejects it
            }
        }
        const autoSubMatches = (await getAutoSubtitleMatches(mediaKey, mediaTitle)).filter(
            (match) => !rememberedSubPaths.has(normalizeLocalPathForCompare(match.path)),
        );
        if (getMediaKey() !== mediaKey) return;
        enqueueBackgroundSubtitles([
            ...autoSubMatches.map((match, index) => ({
                path: match.path,
                title: match.name,
                mode: index === 0 ? "select" as const : "auto" as const,
                mediaKey,
            })),
            ...subPaths.map((path) => ({
                path,
                title: titleFromPath(path),
                mode: "select" as const,
                mediaKey,
            })),
        ]);
    };

    const resetTracks = () => {
        clearBackgroundSubtitleQueue();
        if (tracksUpdateFrame != null && typeof window !== "undefined") {
            window.cancelAnimationFrame(tracksUpdateFrame);
        }
        tracksUpdateFrame = null;
        pendingTracksUpdate = null;
        videoTracks.value = [];
        audioTracks.value = [];
        subTracks.value = [];
        showAudioMenu.value = false;
        showSubMenu.value = false;
        showSubtitleAdvancedSettings.value = false;
        activeOnlineSubtitleProviderId.value = DEFAULT_ONLINE_SUBTITLE_PROVIDER_ID;
        onlineSubtitleResultMap.value = {
            opensubtitles: [],
            subsource: [],
        };
        onlineSubtitleErrorMap.value = {
            opensubtitles: "",
            subsource: "",
        };
        onlineSubtitleLoadingMap.value = {
            opensubtitles: false,
            subsource: false,
        };
        onlineSubtitleSearchedMap.value = {
            opensubtitles: false,
            subsource: false,
        };
        isOnlineSubtitleDialogOpen.value = false;
        isLoadingOnlineSubtitle.value = false;
        onlineSubtitleMediaKey = "";
        onlineSubtitleMediaTitle = undefined;
        onlineSubtitleSearchGeneration += 1;
        subtitleState.resetSubtitleState();
    };

    const setSubtitlesDisabled = async (disabled: boolean) => {
        if (disabled) {
            await subtitleState.disableAllSubtitles();
            subTracks.value.forEach((track) => {
                track.selected = String(track.id) === "0";
            });
            return;
        }
        await subtitleState.enableAutoSubtitleSelection();
    };

    return {
        videoTracks,
        audioTracks,
        subTracks,
        activeOnlineSubtitleProviderId,
        onlineSubtitleProviderTabs,
        onlineSubtitleResults,
        onlineSubtitleErrorMessage,
        isOnlineSubtitleDialogOpen,
        isSearchingOnlineSubtitles,
        isLoadingOnlineSubtitle,
        handleTracksSnapshot,
        showAudioMenu,
        showSubMenu,
        showSubtitleAdvancedSettings,
        dualSubEnabled: subtitleState.dualSubEnabled,
        secondarySubId: subtitleState.secondarySubId,
        activeSubTarget: subtitleState.activeSubTarget,
        selectAudio,
        selectSubTrack,
        setDualSubEnabled,
        setActiveSubTarget: subtitleState.setActiveSubTarget,
        addExternalAudioTrack,
        addExternalSubtitleTrack,
        searchOnlineSubtitleTracks,
        setOnlineSubtitleProvider,
        addSelectedOnlineSubtitleTrack,
        closeOnlineSubtitleDialog,
        applyExternalTracksForUrl,
        setSubtitlesDisabled,
        resetTracks,
    };
};
