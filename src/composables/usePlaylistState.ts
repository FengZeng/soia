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

type PersistedPlaylistState = {
    playlists?: Playlist[];
    playlistLoopMode?: PlaylistLoopMode;
    playlistSortMode?: PlaylistSortMode;
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

const createPlaylistId = () =>
    `pl_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;

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

const normalizePlaylistEntries = (entries: PlaylistEntry[]): PlaylistEntry[] => {
    const unique = new Map<string, PlaylistEntry>();
    entries.forEach((entry) => {
        const path = entry?.path?.trim();
        if (!path) return;
        const title = entry?.title?.trim() || undefined;
        const iconUrl = entry?.iconUrl?.trim() || undefined;
        unique.set(path, {
            coreEntryId: entry.coreEntryId,
            path,
            title,
            iconUrl,
            addedAt:
                typeof entry.addedAt === "number" ? entry.addedAt : Date.now(),
        });
    });
    return Array.from(unique.values());
};

const normalizePlaylistName = (name: string | undefined, fallback: string) => {
    const trimmed = name?.trim();
    return trimmed || fallback;
};

const normalizePlaylists = (items: Playlist[] | undefined): Playlist[] => {
    const source = items ?? [];
    let favoritesPlaylist: Playlist | null = null;
    const userPlaylists: Playlist[] = [];

    source.forEach((item, index) => {
        const normalizedPlaylist: Playlist = {
            id: item.id || createPlaylistId(),
            coreRevision: item.coreRevision,
            name: normalizePlaylistName(item.name, `Playlist ${index + 1}`),
            entries: normalizePlaylistEntries(item.entries ?? []),
            createdAt:
                typeof item.createdAt === "number" ? item.createdAt : Date.now(),
        };

        if (
            normalizedPlaylist.id === FAVORITES_PLAYLIST_ID ||
            normalizedPlaylist.id === LEGACY_FAVOURITE_PLAYLIST_ID
        ) {
            favoritesPlaylist = {
                ...normalizedPlaylist,
                id: FAVORITES_PLAYLIST_ID,
                name: FAVORITES_PLAYLIST_NAME,
                createdAt: normalizedPlaylist.createdAt || 0,
            };
            return;
        }

        userPlaylists.push(normalizedPlaylist);
    });

    return [favoritesPlaylist ?? createFavoritesPlaylist(), ...userPlaylists];
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
    const playbackPlaylistId = ref<string | null>(null);
    const loopMode = ref<PlaylistLoopMode>("list");
    const sortMode = ref<PlaylistSortMode>("added");
    const isLoopOne = ref(false);
    const collectionRevision = ref(0);
    let unsubscribeFromCore: (() => void) | null = null;

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

    const getOrderedEntriesByPlaylistId = (playlistId: string | null) => {
        const target = findPlaylistById(playlistId);
        if (!target) return [];
        return sortEntries(target.entries, sortMode.value);
    };

    const syncSelectionAfterMutation = () => {
        if (!hasPlaylist(activePlaylistId.value)) {
            activePlaylistId.value = null;
        }
        if (!hasPlaylist(playbackPlaylistId.value)) {
            playbackPlaylistId.value = null;
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
        const normalizedEntries = normalizePlaylistEntries(entries);
        if (!normalizedEntries.length) return null;

        const fallbackName = `Playlist ${
            playlists.value.filter((item) => !isFavoritesPlaylist(item.id)).length + 1
        }`;
        const derivedName = derivePlaylistNameFromPaths(
            normalizedEntries.map((item) => item.path),
            fallbackName,
        );
        const created = await tauriPlaylistClient.create({
            name: normalizePlaylistName(options.name, derivedName),
            expectedCollectionRevision: collectionRevision.value,
        });
        const playlistId = created.playlist?.summary.id;
        if (!playlistId || !created.playlist) return null;
        if (normalizedEntries.length) {
            await tauriPlaylistClient.mutate({
                type: "addEntries",
                playlistId,
                entries: normalizedEntries.map((entry) => ({
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
        const snapshot = await tauriPlaylistClient.getSnapshot();
        const summaries = snapshot.playlists;
        loopMode.value = snapshot.loopMode === "shuffle" ? "shuffle" : "list";
        sortMode.value = snapshot.sortMode === "added" ? "added" : "name";
        isLoopOne.value = snapshot.isLoopOne;
        collectionRevision.value = snapshot.collectionRevision;
        const loaded = await Promise.all(summaries.map(async (summary) => {
            const entries = [] as PlaylistEntry[];
            for (let offset = 0; offset < summary.entryCount; offset += 200) {
                const page = await tauriPlaylistClient.getEntriesPage({ playlistId: summary.id, offset, limit: 200 });
                entries.push(...page.entries.map((entry) => ({
                    coreEntryId: entry.id, path: entry.playbackKey, title: entry.title ?? undefined,
                    iconUrl: entry.artworkRef ?? undefined, addedAt: entry.addedAt,
                })));
            }
            return { id: summary.id, coreRevision: summary.revision, name: summary.name, entries, createdAt: summary.createdAt };
        }));
        playlists.value = normalizePlaylists(loaded);
        playbackPlaylistId.value = hasPlaylist(snapshot.playbackPlaylistId)
            ? snapshot.playbackPlaylistId
            : null;
        syncSelectionAfterMutation();
        if (!unsubscribeFromCore) {
            unsubscribeFromCore = tauriPlaylistClient.subscribe(() => {
                void loadFromCore();
            });
        }
    };

    onUnmounted(() => {
        unsubscribeFromCore?.();
        unsubscribeFromCore = null;
    });

    const toPersistedState = () => ({
        playlists: playlists.value,
        playlistLoopMode: loopMode.value,
        playlistSortMode: sortMode.value,
        activePlaylistId: activePlaylistId.value,
    });

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

    const pickRandomIndex = (length: number, currentIndex: number): number => {
        if (length <= 1) return 0;
        let nextIndex = currentIndex;
        do {
            nextIndex = Math.floor(Math.random() * length);
        } while (nextIndex === currentIndex);
        return nextIndex;
    };

    const resolvePlaybackPlaylistId = (currentPath: string): string | null => {
        const current = currentPath.trim();
        if (!current) return null;

        const playbackPlaylist = findPlaylistById(playbackPlaylistId.value);
        if (playbackPlaylist?.entries.some((entry) => entry.path === current)) {
            return playbackPlaylist.id;
        }

        const active = activePlaylist.value;
        if (active?.entries.some((entry) => entry.path === current)) {
            return active.id;
        }

        const matched = [...playlists.value]
            .reverse()
            .find((item) => item.entries.some((entry) => entry.path === current));
        return matched?.id ?? null;
    };

    const getAdjacentPath = (
        currentPath: string,
        direction: 1 | -1,
    ): string | null => {
        const playlistId = resolvePlaybackPlaylistId(currentPath);
        if (!playlistId) return null;
        playbackPlaylistId.value = playlistId;

        const list = getOrderedEntriesByPlaylistId(playlistId);
        if (!list.length) return null;
        const currentIndex = list.findIndex((item) => item.path === currentPath);

        if (loopMode.value === "shuffle") {
            return list[pickRandomIndex(list.length, currentIndex)]?.path ?? null;
        }

        if (currentIndex < 0) {
            return direction === 1
                ? list[0]?.path ?? null
                : list[list.length - 1]?.path ?? null;
        }

        let nextIndex = currentIndex + direction;
        if (nextIndex < 0) nextIndex = list.length - 1;
        if (nextIndex >= list.length) nextIndex = 0;
        return list[nextIndex]?.path ?? null;
    };

    const getPathForEnd = (currentPath: string): string | null => {
        if (isLoopOne.value) return null;

        const playlistId = resolvePlaybackPlaylistId(currentPath);
        if (!playlistId) return null;
        playbackPlaylistId.value = playlistId;

        const list = getOrderedEntriesByPlaylistId(playlistId);
        if (!list.length) return null;
        const currentIndex = list.findIndex((item) => item.path === currentPath);
        if (currentIndex < 0) return null;

        if (loopMode.value === "shuffle") {
            return list[pickRandomIndex(list.length, currentIndex)]?.path ?? null;
        }

        return list[(currentIndex + 1) % list.length]?.path ?? null;
    };

    const getTitleForPath = (path: string): string | undefined => {
        const normalizedPath = path.trim();
        if (!normalizedPath) return undefined;
        const playlistId = resolvePlaybackPlaylistId(normalizedPath);
        if (!playlistId) return undefined;
        const entry = getOrderedEntriesByPlaylistId(playlistId).find(
            (item) => item.path === normalizedPath,
        );
        return entry?.title?.trim() || undefined;
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
        backToPlaylistList,
        markActivePlaylistAsPlayback,
        cycleSortMode,
        getAdjacentPath,
        getPathForEnd,
        getTitleForPath,
        toggleLoopOne,
        togglePlaylistLoop,
    };
};
