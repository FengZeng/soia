import { computed, nextTick, type Ref, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type {
    NetworkBrowseEntry,
    NetworkBrowseResult,
    NetworkFileRow,
    NetworkConnection,
} from "../types/network";

type PathCrumb = {
    label: string;
    path: string;
};

type DlnaNavigationState = {
    parentById: Map<string, string>;
    nameById: Map<string, string>;
};

export type NetworkSortField = "name" | "added";
export type NetworkSortDirection = "asc" | "desc";

const normalizePath = (path: string) => {
    const trimmed = path.trim();
    if (!trimmed || trimmed === "/") return "/";
    const withLeading = trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
    return withLeading.replace(/\/+$/, "");
};

const getParentPath = (path: string) => {
    if (!path.startsWith("/")) return null;
    const normalized = normalizePath(path);
    if (normalized === "/") return null;
    const index = normalized.lastIndexOf("/");
    if (index < 0) return null;
    if (index === 0) return "/";
    return normalized.slice(0, index);
};

const normalizeObjectId = (value: string) => {
    const trimmed = value.trim();
    return (trimmed.replace(/^\/+/, "") || "0");
};

const formatSize = (bytes: number | null) => {
    if (bytes == null || Number.isNaN(bytes)) return "—";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
        value /= 1024;
        unit += 1;
    }
    return unit === 0 ? `${Math.round(value)} ${units[unit]}` : `${value.toFixed(1)} ${units[unit]}`;
};

const formatModified = (value: string | null) => value ?? "";

const mapBrowseEntry = (entry: NetworkBrowseEntry): NetworkFileRow => ({
    name: entry.name,
    path: entry.path,
    type: entry.entryType === "dir" ? "DIR" : "FILE",
    playbackKey: entry.playbackKey ?? undefined,
    size: formatSize(entry.size),
    modified: formatModified(entry.modifiedAt),
    modifiedAt: entry.modifiedAt,
    createdAt: entry.createdAt,
});

export const useNetworkBrowser = (
    selectedConnectionId: Ref<string>,
    selectedConnection: Ref<NetworkConnection | null>,
) => {
    const networkPath = ref("/");
    const currentFiles = ref<NetworkFileRow[]>([]);
    const isLoading = ref(false);
    const errorMessage = ref("");
    const sortField = ref<NetworkSortField>("name");
    const sortDirection = ref<NetworkSortDirection>("asc");
    const browseCache = new Map<string, Map<string, NetworkFileRow[]>>();
    const dlnaNavigationByConnection = new Map<string, DlnaNavigationState>();

    const getConnectionCache = (connectionId: string) => {
        let cache = browseCache.get(connectionId);
        if (!cache) {
            cache = new Map();
            browseCache.set(connectionId, cache);
        }
        return cache;
    };

    const readCache = (connectionId: string, path: string) => {
        const cache = browseCache.get(connectionId);
        if (!cache) return null;
        return cache.get(path) ?? null;
    };

    const writeCache = (connectionId: string, path: string, entries: NetworkFileRow[]) => {
        const cache = getConnectionCache(connectionId);
        cache.set(path, entries);
    };

    const getDlnaNavigation = (connectionId: string) => {
        let state = dlnaNavigationByConnection.get(connectionId);
        if (!state) {
            state = {
                parentById: new Map(),
                nameById: new Map([["0", "Root"]]),
            };
            dlnaNavigationByConnection.set(connectionId, state);
        }
        return state;
    };

    const trackDlnaNavigation = (
        connectionId: string,
        currentId: string,
        entries: NetworkBrowseEntry[],
    ) => {
        const state = getDlnaNavigation(connectionId);
        if (!state.nameById.has(currentId)) {
            state.nameById.set(currentId, currentId === "0" ? "Root" : currentId);
        }
        for (const entry of entries) {
            const childId = normalizeObjectId(entry.path);
            state.parentById.set(childId, currentId);
            state.nameById.set(childId, entry.name);
        }
    };

    const browse = async (
        path: string,
        mode: "connect" | "browse",
        options: { allowCache?: boolean } = {},
    ) => {
        const connectionId = selectedConnectionId.value;
        if (!connectionId) return;
        const protocol = selectedConnection.value?.protocol?.toLowerCase() ?? "webdav";
        const isDlna = protocol === "http-dlna" || protocol === "dlna";
        const targetPath = isDlna ? normalizeObjectId(path) : normalizePath(path);
        const allowCache = Boolean(options.allowCache);
        if (allowCache) {
            const cached = readCache(connectionId, targetPath);
            if (cached) {
                networkPath.value = targetPath;
                currentFiles.value = cached.map((entry) => ({ ...entry }));
                errorMessage.value = "";
                return;
            }
        }
        const previousPath = networkPath.value;
        const previousFiles = [...currentFiles.value];
        const isDirectoryBrowse = mode === "browse";
        const isConnect = mode === "connect";
        const shouldOptimisticNavigate = isDirectoryBrowse || isConnect;

        if (shouldOptimisticNavigate) {
            // Optimistically enter target directory for immediate UI feedback.
            networkPath.value = targetPath;
            currentFiles.value = [];
        }

        isLoading.value = true;
        errorMessage.value = "";
        await nextTick();
        try {
            const result = await invoke<NetworkBrowseResult>("browse_network_connection", {
                payload: {
                    connectionId,
                    mode,
                    protocol,
                    path: targetPath,
                },
            });
            networkPath.value = isDlna
                ? normalizeObjectId(result.path)
                : normalizePath(result.path);
            if (isDlna) {
                trackDlnaNavigation(connectionId, networkPath.value, result.entries);
            }
            currentFiles.value = result.entries.map(mapBrowseEntry);
            writeCache(connectionId, networkPath.value, currentFiles.value);
        } catch (error) {
            const message =
                error instanceof Error
                    ? error.message
                    : typeof error === "string"
                      ? error
                      : "Network browse failed";
            errorMessage.value = message;
            if (isDirectoryBrowse) {
                networkPath.value = previousPath;
                currentFiles.value = previousFiles;
            } else {
                currentFiles.value = [];
            }
            throw error;
        } finally {
            isLoading.value = false;
        }
    };

    const connect = async () => browse(networkPath.value, "connect");

    const refresh = async () => browse(networkPath.value, "browse");

    const openDirectory = async (path: string) => {
        await browse(path, "browse", { allowCache: true });
    };

    const getAncestorPaths = (path: string) => {
        const connectionId = selectedConnectionId.value;
        if (!connectionId) return [] as string[];
        const protocol = selectedConnection.value?.protocol?.toLowerCase() ?? "webdav";
        const isDlna = protocol === "http-dlna" || protocol === "dlna";

        if (!isDlna) {
            const ancestors: string[] = [];
            let cursor = getParentPath(path);
            while (cursor) {
                ancestors.push(cursor);
                cursor = getParentPath(cursor);
            }
            return ancestors;
        }

        const state = getDlnaNavigation(connectionId);
        const ancestors: string[] = [];
        let cursor = state.parentById.get(normalizeObjectId(path));
        while (cursor) {
            ancestors.push(cursor);
            cursor = state.parentById.get(cursor);
        }
        return ancestors;
    };

    const parentPath = computed(() => {
        const connectionId = selectedConnectionId.value;
        if (!connectionId) return null;
        const protocol = selectedConnection.value?.protocol?.toLowerCase() ?? "webdav";
        const isDlna = protocol === "http-dlna" || protocol === "dlna";
        if (!isDlna) {
            return getParentPath(networkPath.value);
        }
        const state = getDlnaNavigation(connectionId);
        return state.parentById.get(normalizeObjectId(networkPath.value)) ?? null;
    });

    const pathCrumbs = computed<PathCrumb[]>(() => {
        const connectionId = selectedConnectionId.value;
        if (!connectionId) return [];
        const protocol = selectedConnection.value?.protocol?.toLowerCase() ?? "webdav";
        const isDlna = protocol === "http-dlna" || protocol === "dlna";
        if (!isDlna) {
            const normalized = networkPath.value || "/";
            if (normalized === "/") return [];
            const segments = normalized.split("/").filter(Boolean);
            const crumbs: PathCrumb[] = [];
            let current = "";
            for (const segment of segments) {
                current += `/${segment}`;
                crumbs.push({ label: segment, path: current });
            }
            return crumbs;
        }

        const state = getDlnaNavigation(connectionId);
        const currentId = normalizeObjectId(networkPath.value);
        if (currentId === "0") return [];
        const chain: string[] = [];
        let cursor: string | undefined = currentId;
        while (cursor && cursor !== "0") {
            chain.push(cursor);
            cursor = state.parentById.get(cursor);
        }
        chain.reverse();
        return chain.map((id) => ({
            label: state.nameById.get(id) ?? id,
            path: id,
        }));
    });

    const sortedEntries = computed(() => {
        const entries = [...currentFiles.value];
        const direction = sortDirection.value === "asc" ? 1 : -1;
        const dateValue = (entry: NetworkFileRow) => {
            // WebDAV exposes creationdate as the closest equivalent to “date
            // added”. Older servers may omit it, so use last-modified as a
            // useful fallback and place entries without either value last.
            const raw = entry.createdAt || entry.modifiedAt || "";
            const value = raw ? Date.parse(raw) : Number.NaN;
            return Number.isFinite(value) ? value : null;
        };
        entries.sort((left, right) => {
            // Keep folders together at the top in every sort mode, matching the
            // existing network-browser behavior.
            if (left.type !== right.type) return left.type === "DIR" ? -1 : 1;

            let comparison = 0;
            if (sortField.value === "added") {
                const leftDate = dateValue(left);
                const rightDate = dateValue(right);
                if (leftDate !== null && rightDate !== null) {
                    comparison = leftDate - rightDate;
                } else if (leftDate !== null) {
                    return -1;
                } else if (rightDate !== null) {
                    return 1;
                }
            }
            if (comparison === 0) {
                comparison = left.name.localeCompare(right.name, undefined, {
                    sensitivity: "base",
                    numeric: true,
                });
            }
            return comparison * direction;
        });
        return entries;
    });

    const networkEntries = computed(() => {
        const entries = [...sortedEntries.value];
        if (parentPath.value) {
            entries.unshift({
                name: "..",
                path: parentPath.value,
                type: "DIR",
                size: "—",
                modified: "",
                isParent: true,
            });
        }
        return entries;
    });

    const hasFiles = computed(() => currentFiles.value.length > 0);

    return {
        networkPath,
        currentFiles,
        networkEntries,
        pathCrumbs,
        parentPath,
        isLoading,
        errorMessage,
        sortField,
        sortDirection,
        sortedEntries,
        hasFiles,
        connect,
        refresh,
        openDirectory,
        getAncestorPaths,
        normalizePath,
        getParentPath,
    };
};
