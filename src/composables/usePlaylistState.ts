import { computed, onUnmounted, ref } from "vue";
import { tauriPlaylistClient } from "../core-client/tauriPlaylistClient";
import {
    FAVORITES_PLAYLIST_ID,
    FAVORITES_PLAYLIST_NAME,
    LEGACY_FAVOURITE_PLAYLIST_ID,
    type Playlist,
    type PlaylistEntry,
    type PlaylistLoopMode,
    type PlaylistSortMode,
} from "../types/playlist";
import { getPathDisplayName } from "../utils/getPathDisplayName";
import { createPlaylistReaderController } from "../features/playlist/playlistReaderController";

type PersistedPlaylistState = {
    activePlaylistId?: string | null;
};

type CreatePlaylistOptions = {
    name?: string;
    openInDrawer?: boolean;
    setAsPlayback?: boolean;
};

type CreatePlaylistEntryInput = {
    path: string;
    title?: string;
    iconUrl?: string;
};

const isFavoritesPlaylist = (playlistId: string | null) =>
    playlistId === FAVORITES_PLAYLIST_ID;

const normalizePlaylistId = (playlistId: string | null | undefined) =>
    playlistId === LEGACY_FAVOURITE_PLAYLIST_ID
        ? FAVORITES_PLAYLIST_ID
        : playlistId ?? null;

const createFavoritesPlaylist = (): Playlist => ({
    id: FAVORITES_PLAYLIST_ID,
    name: FAVORITES_PLAYLIST_NAME,
    entries: [],
    createdAt: 0,
});

const stripExtension = (fileName: string) => {
    const trimmed = fileName.trim();
    if (!trimmed) return "";
    const dotIndex = trimmed.lastIndexOf(".");
    if (dotIndex <= 0) return trimmed;
    return trimmed.slice(0, dotIndex);
};

const getParentDirectoryName = (path: string) => {
    const parts = path.split(/[/\\]+/).filter(Boolean);
    if (parts.length < 2) return "";
    return parts[parts.length - 2] ?? "";
};

const isNumericName = (value: string) => /^\d+$/.test(value.trim());

const getCommonPrefix = (values: string[]) => {
    if (!values.length) return "";
    let prefix = values[0] ?? "";
    for (let index = 1; index < values.length; index += 1) {
        const current = values[index] ?? "";
        while (prefix && !current.startsWith(prefix)) {
            prefix = prefix.slice(0, -1);
        }
        if (!prefix) return "";
    }
    return prefix.replace(/[\s._-]+$/, "").trim();
};

const derivePlaylistNameFromPaths = (paths: string[], fallback: string) => {
    const fileNames = paths
        .map((path) => getPathDisplayName(path).trim())
        .filter(Boolean);
    const itemNames = fileNames.map(stripExtension).filter(Boolean);
    if (!itemNames.length) return fallback;

    if (itemNames.every(isNumericName)) {
        const folderNames = paths
            .map((path) => getParentDirectoryName(path).trim())
            .filter(Boolean);
        const uniqueFolderNames = Array.from(new Set(folderNames));
        if (uniqueFolderNames.length === 1) {
            return uniqueFolderNames[0] ?? fallback;
        }
        if (uniqueFolderNames.length > 1) {
            return uniqueFolderNames[0] ?? fallback;
        }
    }

    if (itemNames.length === 1) return itemNames[0] ?? fallback;

    const commonPrefix = getCommonPrefix(itemNames);
    if (commonPrefix.length >= 2) return commonPrefix;

    return itemNames[0] ?? fallback;
};

const normalizePlaylistName = (name: string | undefined, fallback: string) => {
    const trimmed = name?.trim();
    return trimmed || fallback;
};

const sortEntries = (
    entries: PlaylistEntry[],
    mode: PlaylistSortMode,
): PlaylistEntry[] => {
    const list = [...entries];
    if (mode === "name") {
        list.sort((a, b) =>
            (a.title?.trim() || getPathDisplayName(a.path)).localeCompare(
                b.title?.trim() || getPathDisplayName(b.path),
                undefined,
                { numeric: true, sensitivity: "base" },
            ),
        );
    } else {
        list.sort((a, b) => b.addedAt - a.addedAt);
    }
    return list;
};

export const usePlaylistState = () => {
    const playlists = ref<Playlist[]>([createFavoritesPlaylist()]);
    const activePlaylistId = ref<string | null>(null);
    const loopMode = ref<PlaylistLoopMode>("list");
    const sortMode = ref<PlaylistSortMode>("added");
    const isLoopOne = ref(false);
    const collectionRevision = ref(0);
    let unsubscribeFromCore: (() => void) | null = null;
    const playlistReader = createPlaylistReaderController(tauriPlaylistClient);

    const activePlaylist = computed<Playlist | null>(
        () =>
            playlists.value.find((item) => item.id === activePlaylistId.value) ??
            null,
    );
    const playlist = computed<PlaylistEntry[]>(() => activePlaylist.value?.entries ?? []);
    const orderedPlaylist = computed(() =>
        sortEntries(playlist.value, sortMode.value),
    );

    const hasPlaylist = (playlistId: string | null) =>
        !!playlistId && playlists.value.some((item) => item.id === playlistId);

    const findPlaylistById = (playlistId: string | null): Playlist | null => {
        if (!playlistId) return null;
        return playlists.value.find((item) => item.id === playlistId) ?? null;
    };

    const syncSelectionAfterMutation = () => {
        if (!hasPlaylist(activePlaylistId.value)) {
            activePlaylistId.value = null;
        }
    };

    const addManyToPlaylist = async (playlistId: string, paths: string[]) => {
        const target = findPlaylistById(playlistId);
        if (!target?.coreRevision) return;
        const existing = new Set(target.entries.map((item) => item.path));
        const dedupedPaths = Array.from(
            new Set(paths.map((item) => item.trim()).filter(Boolean)),
        );
        const additions = dedupedPaths
            .filter((path) => !existing.has(path))
            .map((path) => ({ playbackKey: path, title: null, artworkRef: null }));
        if (!additions.length) return;
        await tauriPlaylistClient.mutate({
            type: "addEntries", playlistId, entries: additions,
            expectedPlaylistRevision: target.coreRevision,
        });
        await loadFromCore();
    };

    const addEntryToPlaylist = async (
        playlistId: string,
        item: CreatePlaylistEntryInput,
    ) => {
        const target = findPlaylistById(playlistId);
        if (!target?.coreRevision) return;

        const path = item.path?.trim() ?? "";
        if (!path) return;

        await tauriPlaylistClient.mutate({
            type: "addEntries", playlistId,
            entries: [{ playbackKey: path, title: item.title?.trim() || null, artworkRef: item.iconUrl?.trim() || null }],
            expectedPlaylistRevision: target.coreRevision,
        });
        await loadFromCore();
    };

    const addToFavorites = async (item: CreatePlaylistEntryInput) => {
        await addEntryToPlaylist(FAVORITES_PLAYLIST_ID, item);
    };

    const createPlaylistWithEntries = async (
        items: CreatePlaylistEntryInput[],
        options: CreatePlaylistOptions = {},
    ): Promise<string | null> => {
        const timestamp = Date.now();
        const normalizedItems = items
            .map((item) => ({
                path: item.path?.trim() ?? "",
                title: item.title?.trim() || undefined,
                iconUrl: item.iconUrl?.trim() || undefined,
            }))
            .filter((item) => !!item.path);
        const entries = normalizedItems.map((item, index) => ({
            path: item.path,
            title: item.title,
            iconUrl: item.iconUrl,
            addedAt: timestamp + index,
        }));
        if (!entries.length) return null;

        const fallbackName = `Playlist ${
            playlists.value.filter((item) => !isFavoritesPlaylist(item.id)).length + 1
        }`;
        const derivedName = derivePlaylistNameFromPaths(
            entries.map((item) => item.path),
            fallbackName,
        );
        const created = await tauriPlaylistClient.create({
            name: normalizePlaylistName(options.name, derivedName),
            expectedCollectionRevision: collectionRevision.value,
        });
        const playlistId = created.playlist?.summary.id;
        if (!playlistId || !created.playlist) return null;
        if (entries.length) {
            await tauriPlaylistClient.mutate({
                type: "addEntries",
                playlistId,
                entries: entries.map((entry) => ({
                    playbackKey: entry.path,
                    title: entry.title ?? null,
                    artworkRef: entry.iconUrl ?? null,
                })),
                expectedPlaylistRevision: created.playlist.summary.revision,
            });
        }
        if (options.openInDrawer) {
            activePlaylistId.value = playlistId;
        }
        if (options.setAsPlayback) {
            await tauriPlaylistClient.mutate({
                type: "setPlaybackPlaylist",
                playlistId,
            });
        }
        await loadFromCore();
        return playlistId;
    };

    const getDefaultPlaylistNameForEntries = (
        items: CreatePlaylistEntryInput[],
        fallback?: string,
    ) => {
        const normalizedPaths = items
            .map((item) => item.path?.trim() ?? "")
            .filter(Boolean);
        const fallbackName =
            fallback?.trim() ||
            `Playlist ${
                playlists.value.filter((item) => !isFavoritesPlaylist(item.id)).length +
                1
            }`;
        return derivePlaylistNameFromPaths(normalizedPaths, fallbackName);
    };

    const getDefaultPlaylistNameForPaths = (paths: string[], fallback?: string) =>
        getDefaultPlaylistNameForEntries(
            paths.map((path) => ({ path })),
            fallback,
        );

    const createPlaylistWithPaths = async (
        paths: string[],
        options: CreatePlaylistOptions = {},
    ): Promise<string | null> =>
        await createPlaylistWithEntries(
            paths.map((path) => ({ path })),
            options,
        );

    const applyPersistedState = (stored: PersistedPlaylistState) => {
        const storedActivePlaylistId = normalizePlaylistId(stored.activePlaylistId);
        activePlaylistId.value = hasPlaylist(storedActivePlaylistId)
            ? storedActivePlaylistId
            : null;
        syncSelectionAfterMutation();
    };

    const loadFromCore = async () => {
        const snapshot = await playlistReader.getSnapshot();
        const summaries = snapshot.playlists;
        loopMode.value = snapshot.loopMode === "shuffle" ? "shuffle" : "list";
        sortMode.value = snapshot.sortMode === "added" ? "added" : "name";
        isLoopOne.value = snapshot.isLoopOne;
        collectionRevision.value = snapshot.collectionRevision;
        const loaded = await Promise.all(summaries.map(async (summary) => {
            const entries = [] as PlaylistEntry[];
            for (let offset = 0; offset < summary.entryCount; offset += 200) {
                const page = await playlistReader.getEntriesPage(summary.id, offset, 200);
                entries.push(...page.entries.map((entry) => ({
                    coreEntryId: entry.id, path: entry.playbackKey, title: entry.title ?? undefined,
                    iconUrl: entry.artworkRef ?? undefined, addedAt: entry.addedAt,
                })));
            }
            return { id: summary.id, coreRevision: summary.revision, name: summary.name, entries, createdAt: summary.createdAt };
        }));
        playlists.value = loaded;
        syncSelectionAfterMutation();
        if (!unsubscribeFromCore) {
            unsubscribeFromCore = playlistReader.subscribe(() => {
                void loadFromCore();
            });
        }
    };

    onUnmounted(() => {
        unsubscribeFromCore?.();
        playlistReader.dispose();
        unsubscribeFromCore = null;
    });

    const toPersistedState = () => ({ activePlaylistId: activePlaylistId.value });

    const addFromDrawerSelection = async (paths: string[]) => {
        if (activePlaylist.value) {
            await addManyToPlaylist(activePlaylist.value.id, paths);
            return;
        }
        await createPlaylistWithPaths(paths, { openInDrawer: true });
    };

    const clearActivePlaylist = async () => {
        const target = activePlaylist.value;
        if (!target?.coreRevision) return;
        await tauriPlaylistClient.mutate({ type: "clear", playlistId: target.id, expectedPlaylistRevision: target.coreRevision });
        await loadFromCore();
    };

    const removeFromActivePlaylist = async (entry: PlaylistEntry) => {
        const target = activePlaylist.value;
        if (!target?.coreRevision || !entry.coreEntryId) return;
        await tauriPlaylistClient.mutate({ type: "removeEntries", playlistId: target.id, entryIds: [entry.coreEntryId], expectedPlaylistRevision: target.coreRevision });
        await loadFromCore();
    };

    const renamePlaylist = async (playlistId: string, name: string) => {
        const target = findPlaylistById(playlistId);
        if (isFavoritesPlaylist(playlistId) || !target?.coreRevision) return;
        const normalizedName = name.trim();
        if (!normalizedName) return;
        await tauriPlaylistClient.mutate({ type: "rename", playlistId, name: normalizedName, expectedPlaylistRevision: target.coreRevision });
        await loadFromCore();
    };

    const deletePlaylist = async (playlistId: string) => {
        const target = findPlaylistById(playlistId);
        if (isFavoritesPlaylist(playlistId) || !target?.coreRevision) return;
        await tauriPlaylistClient.mutate({ type: "delete", playlistId, expectedPlaylistRevision: target.coreRevision, expectedCollectionRevision: collectionRevision.value });
        await loadFromCore();
    };

    const movePlaylist = async (fromPlaylistId: string, toPlaylistId: string) => {
        if (isFavoritesPlaylist(fromPlaylistId) || isFavoritesPlaylist(toPlaylistId)) {
            return;
        }
        if (fromPlaylistId === toPlaylistId) return;
        const fromIndex = playlists.value.findIndex(
            (item) => item.id === fromPlaylistId,
        );
        const toIndex = playlists.value.findIndex((item) => item.id === toPlaylistId);
        if (fromIndex < 0 || toIndex < 0) return;
        if (Math.abs(fromIndex - toIndex) !== 1) return;

        const nextPlaylists = [...playlists.value];
        const temp = nextPlaylists[fromIndex];
        nextPlaylists[fromIndex] = nextPlaylists[toIndex];
        nextPlaylists[toIndex] = temp;
        await tauriPlaylistClient.mutate({
            type: "reorderPlaylists",
            playlistIds: nextPlaylists.map((item) => item.id),
            expectedCollectionRevision: collectionRevision.value,
        });
        await loadFromCore();
    };

    const enterPlaylist = (playlistId: string) => {
        if (!hasPlaylist(playlistId)) return;
        activePlaylistId.value = playlistId;
    };

    const openPlaylist = async (playlistId: string) => {
        await loadFromCore();
        enterPlaylist(playlistId);
    };

    const backToPlaylistList = () => {
        activePlaylistId.value = null;
    };

    const cycleSortMode = async () => {
        const next = sortMode.value === "name" ? "added" : "name";
        await tauriPlaylistClient.mutate({ type: "setSortMode", sortMode: next });
        await loadFromCore();
    };

    const cycleLoopMode = async () => {
        const next = loopMode.value === "list" ? "shuffle" : "list";
        await tauriPlaylistClient.mutate({ type: "setLoopMode", loopMode: next });
        await loadFromCore();
    };

    const markActivePlaylistAsPlayback = async () => {
        if (!activePlaylist.value) return;
        await tauriPlaylistClient.mutate({ type: "setPlaybackPlaylist", playlistId: activePlaylist.value.id });
        await loadFromCore();
    };

    const toggleLoopOne = async (
        setLoopFile: (enabled: boolean) => Promise<void>,
    ) => {
        const next = !isLoopOne.value;
        await tauriPlaylistClient.mutate({ type: "setLoopOne", isLoopOne: next });
        await setLoopFile(next);
        await loadFromCore();
    };

    const togglePlaylistLoop = async (
        setLoopFile: (enabled: boolean) => Promise<void>,
    ) => {
        if (isLoopOne.value) {
            await tauriPlaylistClient.mutate({ type: "setLoopOne", isLoopOne: false });
            await setLoopFile(false);
        }
        await cycleLoopMode();
    };

    return {
        playlists,
        activePlaylistId,
        activePlaylist,
        playlist,
        loopMode,
        sortMode,
        isLoopOne,
        orderedPlaylist,
        applyPersistedState,
        loadFromCore,
        toPersistedState,
        createPlaylistWithPaths,
        createPlaylistWithEntries,
        getDefaultPlaylistNameForPaths,
        getDefaultPlaylistNameForEntries,
        addFromDrawerSelection,
        addToFavorites,
        clearActivePlaylist,
        removeFromActivePlaylist,
        renamePlaylist,
        deletePlaylist,
        movePlaylist,
        enterPlaylist,
        openPlaylist,
        backToPlaylistList,
        markActivePlaylistAsPlayback,
        cycleSortMode,
        toggleLoopOne,
        togglePlaylistLoop,
    };
};
