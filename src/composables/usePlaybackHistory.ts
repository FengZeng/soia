import { onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { HistoryEntry } from "../types/history";
import { normalizePlaybackKey } from "../utils/playbackDisplay";

const MAX_HISTORY = 100;
const SAVE_DEBOUNCE_MS = 800;

const normalizeTrackList = (value: unknown): string[] => {
    if (!Array.isArray(value)) return [];
    const deduped = new Set<string>();
    value.forEach((entry) => {
        if (typeof entry !== "string") return;
        const trimmed = entry.trim();
        if (!trimmed) return;
        deduped.add(trimmed);
    });
    return Array.from(deduped);
};

const normalizeHistory = (entries: HistoryEntry[]): HistoryEntry[] => {
    const map = new Map<string, HistoryEntry>();
    entries.forEach((entry) => {
        if (!entry?.path) return;
        const normalizedPath = normalizePlaybackKey(entry.path);
        const normalizedTitle =
            typeof entry.title === "string" ? entry.title.trim() : "";
        const normalized: HistoryEntry = {
            id:
                typeof entry.id === "string" && entry.id.trim()
                    ? entry.id.trim()
                    : undefined,
            path: normalizedPath,
            title: normalizedTitle,
            lastPosition: Number.isFinite(entry.lastPosition)
                ? entry.lastPosition
                : 0,
            duration:
                Number.isFinite(entry.duration) && entry.duration > 0
                    ? entry.duration
                    : 0,
            lastPlayedAt: Number.isFinite(entry.lastPlayedAt)
                ? entry.lastPlayedAt
                : 0,
            isPinned: Boolean(entry.isPinned),
            isLivePlayback: Boolean(entry.isLivePlayback),
            externalAudioTracks: normalizeTrackList(entry.externalAudioTracks),
            externalSubTracks: normalizeTrackList(entry.externalSubTracks),
        };
        map.set(normalizedPath, normalized);
    });
    return Array.from(map.values())
        .sort((a, b) => {
            if (a.isPinned !== b.isPinned) {
                return a.isPinned ? -1 : 1;
            }
            return b.lastPlayedAt - a.lastPlayedAt;
        })
        .slice(0, MAX_HISTORY);
};

export const usePlaybackHistory = () => {
    const history = ref<HistoryEntry[]>([]);
    const isReady = ref(false);
    let saveTimer: number | null = null;
    let pendingProgressEntry: HistoryEntry | null = null;

    const clearSaveTimer = () => {
        if (saveTimer) {
            window.clearTimeout(saveTimer);
            saveTimer = null;
        }
    };

    const saveHistory = async () => {
        await invoke("save_play_history", { entries: history.value });
    };

    const saveHistoryEntry = async (entry: HistoryEntry) => {
        await invoke("save_play_history_entry", { entry });
    };

    const stageHistoryEntry = async (entry: HistoryEntry) => {
        await invoke("stage_play_history_entry", { entry });
    };

    const clearStagedHistoryEntry = async (path?: string) => {
        await invoke("clear_staged_play_history_entry", { path: path ?? null });
    };

    const scheduleSave = () => {
        clearSaveTimer();
        saveTimer = window.setTimeout(() => {
            void saveHistory();
            saveTimer = null;
        }, SAVE_DEBOUNCE_MS);
    };

    const stagePendingProgress = async () => {
        const entry = pendingProgressEntry;
        if (!entry) return;
        pendingProgressEntry = null;
        await stageHistoryEntry(entry);
    };

    const discardPendingProgress = (path?: string) => {
        if (!pendingProgressEntry) return;
        if (path && pendingProgressEntry.path !== path) return;
        pendingProgressEntry = null;
    };

    const discardStagedProgress = (path?: string) => {
        discardPendingProgress(path);
        void clearStagedHistoryEntry(path);
    };

    const loadHistory = async () => {
        try {
            const entries = await invoke<HistoryEntry[]>("load_play_history");
            history.value = normalizeHistory(entries ?? []);
        } catch {
            history.value = [];
        } finally {
            isReady.value = true;
        }
    };

    const clearHistory = () => {
        history.value = [];
        discardStagedProgress();
        scheduleSave();
    };

    const removeEntry = (path: string) => {
        const normalizedPath = normalizePlaybackKey(path);
        if (!normalizedPath) return;
        history.value = history.value.filter(
            (entry) => entry.path !== normalizedPath,
        );
        discardStagedProgress(normalizedPath);
        scheduleSave();
    };

    const upsertEntry = (entry: HistoryEntry) => {
        const next = normalizeHistory([
            entry,
            ...history.value.filter((item) => item.path !== entry.path),
        ]);
        history.value = next;
    };

    const findEntry = (path: string): HistoryEntry | undefined =>
        history.value.find((item) => item.path === path);

    const createEntry = (
        path: string,
        position: number,
        duration = 0,
        isPinned = false,
        isLivePlayback = false,
        externalAudioTracks: string[] = [],
        externalSubTracks: string[] = [],
        title = "",
        id?: string,
    ): HistoryEntry => ({
        id,
        path,
        title,
        lastPosition: Math.max(0, position),
        duration: Number.isFinite(duration) && duration > 0 ? duration : 0,
        lastPlayedAt: Date.now(),
        isPinned,
        isLivePlayback,
        externalAudioTracks,
        externalSubTracks,
    });

    const createPlaybackEntry = (
        path: string,
        position: number,
        duration: number,
        title: string,
        isLivePlayback: boolean,
        existing?: HistoryEntry,
    ): HistoryEntry =>
        createEntry(
            path,
            position,
            duration,
            existing?.isPinned ?? false,
            resolveIsLivePlayback(existing, isLivePlayback),
            normalizeTrackList(existing?.externalAudioTracks),
            normalizeTrackList(existing?.externalSubTracks),
            title.trim() || existing?.title || "",
            existing?.id,
        );

    const resolveIsLivePlayback = (
        existing: HistoryEntry | undefined,
        isLivePlayback: boolean,
    ) => isLivePlayback || existing?.isLivePlayback === true;

    const queueProgressForSave = (entry: HistoryEntry) => {
        pendingProgressEntry = entry;
    };

    const updateProgressInMemory = (
        path: string,
        position: number,
        duration: number,
        title: string,
        isLivePlayback: boolean,
    ): HistoryEntry | null => {
        const existing = findEntry(path);
        const resolvedPosition =
            position > 0 ? position : existing?.lastPosition ?? 0;
        if (resolvedPosition <= 0) return null;

        const entry = createPlaybackEntry(
            path,
            resolvedPosition,
            duration,
            title,
            isLivePlayback,
            existing,
        );
        entry.lastPlayedAt = existing?.lastPlayedAt ?? entry.lastPlayedAt;
        upsertEntry(entry);
        return entry;
    };

    const markPlaybackStarted = (
        path: string,
        position: number,
        duration = 0,
        title = "",
        isLivePlayback = false,
    ) => {
        if (!path) return;
        const normalizedPath = normalizePlaybackKey(path);
        const existing = findEntry(normalizedPath);
        const entry = createPlaybackEntry(
            normalizedPath,
            existing?.lastPosition ?? position,
            duration,
            title,
            isLivePlayback,
            existing,
        );
        entry.lastPlayedAt = Date.now();
        upsertEntry(entry);
    };

    const recordProgress = (
        path: string,
        position: number,
        duration: number,
        isPlaying: boolean,
        title = "",
        isLivePlayback = false,
    ) => {
        if (!path || !isPlaying) return;
        if (!Number.isFinite(position)) return;
        const normalizedPath = normalizePlaybackKey(path);
        if (!normalizedPath) return;
        const entry = updateProgressInMemory(
            normalizedPath,
            position,
            duration,
            title,
            isLivePlayback,
        );
        if (!entry) return;

        queueProgressForSave(entry);
        void stageHistoryEntry(entry);
    };

    const recordStop = async (
        path: string,
        position: number,
        duration = 0,
        title = "",
        isLivePlayback = false,
    ) => {
        if (!path) return;
        const normalizedPath = normalizePlaybackKey(path);
        if (!normalizedPath) return;
        const pendingEntry =
            pendingProgressEntry?.path === normalizedPath
                ? pendingProgressEntry
                : undefined;
        const existing = findEntry(normalizedPath);
        const resolvedPosition =
            Number.isFinite(position) && position > 0
                ? position
                : pendingEntry?.lastPosition ?? existing?.lastPosition ?? 0;
        const entry = updateProgressInMemory(
            normalizedPath,
            resolvedPosition,
            duration,
            title,
            isLivePlayback,
        );
        if (!entry) return;
        discardPendingProgress(normalizedPath);
        await stageHistoryEntry(entry);
    };

    const updateTitle = (path: string, title: string) => {
        const normalizedPath = normalizePlaybackKey(path);
        const normalizedTitle = title.trim();
        if (!normalizedPath || !normalizedTitle) return;
        const existing = findEntry(normalizedPath);
        if (!existing || existing.title === normalizedTitle) return;
        const nextEntry: HistoryEntry = {
            ...existing,
            title: normalizedTitle,
        };
        discardStagedProgress(normalizedPath);
        upsertEntry(nextEntry);
        void saveHistoryEntry(nextEntry);
    };

    const togglePinned = (path: string) => {
        const normalizedPath = normalizePlaybackKey(path);
        const target = findEntry(normalizedPath);
        if (!target) return;
        const nextEntry: HistoryEntry = {
            ...target,
            isPinned: !target.isPinned,
        };
        discardStagedProgress(normalizedPath);
        upsertEntry(nextEntry);
        void saveHistoryEntry(nextEntry);
    };

    const getExternalTracks = (path: string) => {
        const normalizedPath = normalizePlaybackKey(path);
        const entry = findEntry(normalizedPath);
        return {
            audio: normalizeTrackList(entry?.externalAudioTracks),
            sub: normalizeTrackList(entry?.externalSubTracks),
        };
    };

    const recordExternalTrack = async (
        path: string,
        kind: "audio" | "sub",
        trackPath: string,
    ) => {
        const normalizedPath = normalizePlaybackKey(path);
        const trimmed = trackPath.trim();
        if (!normalizedPath || !trimmed) return;
        const existing = findEntry(normalizedPath);
        const audio = normalizeTrackList(existing?.externalAudioTracks);
        const sub = normalizeTrackList(existing?.externalSubTracks);
        const list = kind === "audio" ? audio : sub;
        if (!list.includes(trimmed)) {
            list.push(trimmed);
        }
        const nextEntry: HistoryEntry = existing
            ? {
                  ...existing,
                  externalAudioTracks: audio,
                  externalSubTracks: sub,
              }
            : {
                  path: normalizedPath,
                  lastPosition: 0,
                  duration: 0,
                  lastPlayedAt: 0,
                  isPinned: false,
                  isLivePlayback: false,
                  externalAudioTracks: audio,
                  externalSubTracks: sub,
              };
        discardStagedProgress(normalizedPath);
        upsertEntry(nextEntry);
        await saveHistoryEntry(nextEntry);
    };

    onMounted(() => {
        void loadHistory();
    });

    onBeforeUnmount(() => {
        void stagePendingProgress();
    });

    return {
        history,
        isReady,
        loadHistory,
        markPlaybackStarted,
        recordProgress,
        recordStop,
        updateTitle,
        clearHistory,
        removeEntry,
        togglePinned,
        getExternalTracks,
        recordExternalTrack,
    };
};
