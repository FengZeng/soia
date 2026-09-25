<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import type { HistoryEntry } from "../types/history";
import type {
    NetworkConnection,
    NetworkFileRow,
    NetworkPlayRequest,
} from "../types/network";
import { normalizePlaybackKey } from "../utils/playbackDisplay";
import {
    resolvePlaybackSource,
    type ResolvedPlaybackSource,
} from "../utils/resolvePlaybackSource";
import { useNetworkPanel } from "../composables/useNetworkPanel";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import CustomSelect from "../components/CustomSelect.vue";
import NetworkConnectionsView from "../components/network/NetworkConnectionsView.vue";
import NetworkBrowserView from "../components/network/NetworkBrowserView.vue";
import NetworkConnectionModal from "../components/network/NetworkConnectionModal.vue";
import { settingsLocale, translateSettingsText } from "../i18n";

const props = defineProps<{
    history: HistoryEntry[];
    currentUrl: string;
    isVisible: boolean;
}>();

const {
    viewMode,
    activeConnectionLabel,
    networkConnections,
    networkEntries,
    networkSortField,
    networkSortDirection,
    setNetworkSort,
    pathCrumbs,
    parentPath,
    selectedConnection,
    selectedConnectionConfig,
    networkPath,
    isLoading,
    connectionStatus,
    errorMessage,
    openEntry,
    onRefresh,
    onSaveConnection,
    onCreateConnection,
    onDeleteConnection,
    onDiscoverConnections,
    onOpenBrowser,
    resetBrowserToRootWhenVisible,
    onBackToConnections,
    onBrowsePath,
    buildPlayRequest,
    getAncestorPaths,
    hasFiles,
} = useNetworkPanel();

const emit = defineEmits<{
    (e: "play-network", payload: NetworkPlayRequest): void;
}>();

const tr = (text: string): string =>
    translateSettingsText(settingsLocale.value, text);

const isCreateModalOpen = ref(false);
const isCreatingConnection = ref(false);
const createError = ref("");
const isDeleteModalOpen = ref(false);
const isDeletingConnection = ref(false);
const deleteError = ref("");
const pendingDeleteConnection = ref<NetworkConnection | null>(null);
const isSwitchingView = ref(false);
const isDiscovering = ref(false);
const editingConnectionId = ref<string | null>(null);
const canDiscoverConnections = true;
const currentResolvedSource = ref<ResolvedPlaybackSource | null>(null);
let resolveCurrentSourceSequence = 0;

watch(
    () => props.isVisible,
    (isVisible, wasVisible) => {
        if (!isVisible || wasVisible) return;
        void resetBrowserToRootWhenVisible();
    },
);

watch(
    () => props.currentUrl,
    async (currentUrl) => {
        const sequence = ++resolveCurrentSourceSequence;
        const trimmed = currentUrl.trim();
        if (!trimmed) {
            currentResolvedSource.value = null;
            return;
        }
        try {
            const source = await resolvePlaybackSource(trimmed);
            if (sequence === resolveCurrentSourceSequence) {
                currentResolvedSource.value = source;
            }
        } catch {
            if (sequence === resolveCurrentSourceSequence) {
                currentResolvedSource.value = null;
            }
        }
    },
    { immediate: true },
);

type SupportedProtocol = "webdav" | "smb" | "ftp" | "http-dlna";

type ProtocolOption = {
    value: SupportedProtocol;
    label: string;
};

const protocolOptions: ProtocolOption[] = [
    { value: "webdav", label: "WebDAV" },
    { value: "smb", label: "SMB" },
    { value: "http-dlna", label: "HTTP DLNA" },
];

const formatPlaybackTime = (value: number) => {
    const totalSeconds = Math.max(0, Math.floor(value || 0));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;

    if (hours > 0) {
        return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
    }
    return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
};

const networkSortValue = computed({
    get: () => `${networkSortField.value}:${networkSortDirection.value}`,
    set: (value: string) => {
        const [field, direction] = value.split(":");
        if (
            (field === "name" || field === "added") &&
            (direction === "asc" || direction === "desc")
        ) {
            setNetworkSort(field, direction);
        }
    },
});

const networkSortOptions = computed(() => [
    { value: "name:asc", label: tr("Alphabetical (A–Z)") },
    { value: "name:desc", label: tr("Alphabetical (Z–A)") },
    { value: "added:desc", label: tr("Date added (newest)") },
    { value: "added:asc", label: tr("Date added (oldest)") },
]);

const createForm = reactive({
    label: "",
    protocol: "webdav" as SupportedProtocol,
    baseUrl: "",
    host: "",
    share: "",
    group: "",
    port: "21",
    username: "",
    password: "",
    defaultPath: "/",
});

const isWebdavProtocol = computed(() => createForm.protocol === "webdav");
const isSmbProtocol = computed(() => createForm.protocol === "smb");
const isFtpProtocol = computed(() => createForm.protocol === "ftp");
const isHttpDlnaProtocol = computed(() => createForm.protocol === "http-dlna");
const requiresAuthFields = computed(
    () =>
        isWebdavProtocol.value || isSmbProtocol.value || isFtpProtocol.value,
);
const serverFieldLabel = computed(() => {
    if (isHttpDlnaProtocol.value) return tr("Device URL");
    return tr("Server URL");
});
const serverFieldPlaceholder = computed(() => {
    if (isHttpDlnaProtocol.value) return "http://192.168.31.66:8200/MediaServer";
    return "http://192.168.31.25:5244/dav";
});
const defaultPathLabel = computed(() => {
    if (isHttpDlnaProtocol.value) return tr("Content Path");
    return tr("Default Path");
});
const selectedProtocolLabel = computed(
    () =>
        protocolOptions.find((option) => option.value === createForm.protocol)?.label ??
        "WebDAV",
);

const resetCreateForm = () => {
    createForm.label = "";
    createForm.protocol = "webdav";
    createForm.baseUrl = "";
    createForm.host = "";
    createForm.share = "";
    createForm.group = "";
    createForm.port = "21";
    createForm.username = "";
    createForm.password = "";
    createForm.defaultPath = "/";
    createError.value = "";
};

const openCreateModal = () => {
    editingConnectionId.value = null;
    resetCreateForm();
    isCreateModalOpen.value = true;
};

const openEditModal = (connection: NetworkConnection) => {
    const protocol = protocolOptions.some(
        (option) => option.value === connection.protocol,
    )
        ? (connection.protocol as SupportedProtocol)
        : "webdav";
    const normalizedBaseUrl = connection.baseUrl || "";
    const normalizedDefaultPath = connection.defaultPath || "/";

    editingConnectionId.value = connection.id;
    createForm.label = connection.label || "WebDAV";
    createForm.protocol = protocol;
    createForm.baseUrl = normalizedBaseUrl;
    createForm.host = "";
    createForm.share = "";
    createForm.group = "";
    createForm.port = "21";
    createForm.username = connection.username || "";
    createForm.password = connection.password || "";
    createForm.defaultPath = normalizedDefaultPath;

    if (protocol === "smb") {
        const username = connection.username || "";
        const [group, user] = username.includes(";")
            ? username.split(/;(.*)/s)
            : ["", username];
        createForm.group = group?.trim() ?? "";
        createForm.username = user?.trim() ?? "";
        const smbMatch = normalizedBaseUrl.match(/^smb:\/\/([^/]+)\/?(.*)$/i);
        if (smbMatch) {
            createForm.host = smbMatch[1] ?? "";
            const sharePath = smbMatch[2] ?? "";
            createForm.share = sharePath.split("/").filter(Boolean)[0] ?? "";
        } else {
            createForm.host = normalizedBaseUrl;
        }
    }

    if (protocol === "ftp") {
        try {
            const parsed = new URL(normalizedBaseUrl);
            createForm.host = parsed.hostname || "";
            createForm.port = parsed.port || "21";
        } catch {
            createForm.host = normalizedBaseUrl.replace(/^ftp:\/\//i, "");
            createForm.port = "21";
        }
    }

    createError.value = "";
    isCreateModalOpen.value = true;
};

const closeCreateModal = () => {
    if (isCreatingConnection.value) return;
    isCreateModalOpen.value = false;
    editingConnectionId.value = null;
    createError.value = "";
};

const closeDeleteModal = (force = false) => {
    if (isDeletingConnection.value && !force) return;
    isDeleteModalOpen.value = false;
    pendingDeleteConnection.value = null;
    deleteError.value = "";
};

const onEntryClick = async (entry: NetworkFileRow) => {
    if (entry.type === "FILE") {
        try {
            emit("play-network", buildPlayRequest(entry));
        } catch (error) {
            maybeOpenSmbCredentialsEditor(error);
        }
        return;
    }
    if (isLoading.value) return;
    try {
        await openEntry(entry);
    } catch (error) {
        maybeOpenSmbCredentialsEditor(error);
    }
};

const onOpenConnectionBrowser = async (connectionId: string) => {
    if (isSwitchingView.value) return;
    isSwitchingView.value = true;
    await nextTick();
    try {
        await onOpenBrowser(connectionId);
        maybeOpenSmbCredentialsEditor(errorMessage.value);
    } finally {
        isSwitchingView.value = false;
    }
};

const visibleNetworkEntries = computed(() =>
    {
        const historyByPath = new Map(
            props.history.map((entry) => [normalizePlaybackKey(entry.path), entry]),
        );
        const currentSource = currentResolvedSource.value;
        const normalizedCurrentUrl = normalizePlaybackKey(
            currentSource?.playbackKey ?? props.currentUrl,
        );
        const selectedProtocol =
            selectedConnectionConfig.value?.protocol?.trim().toLowerCase() ?? "";
        const activeFolderPaths = new Set<string>();

        if (currentSource?.kind === "webdav") {
            const selectedConnectionId = selectedConnectionConfig.value?.id ?? "";
            if (
                selectedProtocol === "webdav" &&
                currentSource.connectionId === selectedConnectionId
            ) {
                getAncestorPaths(currentSource.filePath).forEach((path) =>
                    activeFolderPaths.add(path),
                );
            }
        } else if (currentSource?.kind === "dlna") {
            const selectedConnectionId = selectedConnectionConfig.value?.id ?? "";
            if (
                (selectedProtocol === "http-dlna" || selectedProtocol === "dlna") &&
                currentSource.connectionId === selectedConnectionId
            ) {
                const activeParentPath = currentSource.parentPath;
                if (activeParentPath) {
                    activeFolderPaths.add(activeParentPath);
                    getAncestorPaths(activeParentPath).forEach((path) =>
                        activeFolderPaths.add(path),
                    );
                }
            }
        } else if (currentSource?.kind === "smb") {
            const selectedConnectionId = selectedConnectionConfig.value?.id ?? "";
            if (
                (selectedProtocol === "smb" || selectedProtocol === "samba") &&
                currentSource.connectionId === selectedConnectionId &&
                currentSource.filePath
            ) {
                getAncestorPaths(currentSource.filePath).forEach((path) =>
                    activeFolderPaths.add(path),
                );
            }
        }

        return networkEntries.value
            .filter((entry) => !entry.isParent)
            .map((entry) => {
                if (entry.type !== "FILE") {
                    return {
                        ...entry,
                        playbackProgressText: "",
                        containsActive: activeFolderPaths.has(entry.path),
                    };
                }
                const playbackKey = entry.playbackKey;
                if (!playbackKey) {
                    return {
                        ...entry,
                        playbackProgressText: "",
                        isActive: false,
                        containsActive: false,
                    };
                }
                const historyEntry = historyByPath.get(playbackKey);
                const duration = historyEntry?.duration ?? 0;
                const position = historyEntry?.lastPosition ?? 0;
                const progressText =
                    Number.isFinite(duration) && duration > 0
                        ? `${formatPlaybackTime(position)}/${formatPlaybackTime(duration)}`
                        : "";
                const playbackProgressText = progressText
                    ? `${progressText} | ${entry.size}`
                    : "";
                const isActive = normalizedCurrentUrl === playbackKey;
                return {
                    ...entry,
                    playbackProgressText,
                    isActive,
                    containsActive: false,
                };
            });
    },
);

const onBackFolderClick = async () => {
    if (isLoading.value) return;
    if (!parentPath.value) return;
    await onBrowsePath(parentPath.value);
};

const onPathCrumbClick = async (path: string) => {
    if (isLoading.value) return;
    if (path === networkPath.value) return;
    isPathOverflowMenuOpen.value = false;
    await onBrowsePath(path);
};

const onRefreshClick = async () => {
    await onRefresh();
};

const onConnectionsRefreshClick = async () => {
    if (isDiscovering.value) return;
    isDiscovering.value = true;
    try {
        await onDiscoverConnections();
    } finally {
        isDiscovering.value = false;
    }
};

const shouldShowPathBar = computed(
    () => Boolean(parentPath.value) || pathCrumbs.value.length > 0,
);

type PathCrumb = {
    label: string;
    path: string;
};

const isPathOverflowMenuOpen = ref(false);
const shouldCollapsePathCrumbs = ref(false);
const leadingPathCrumbs = ref<PathCrumb[]>([]);
const pathBarEl = ref<HTMLElement | null>(null);
const pathMeasureEl = ref<HTMLElement | null>(null);
const pathMenuEl = ref<HTMLElement | null>(null);
const pathOverflowEl = ref<HTMLElement | null>(null);
const isPathMenuAlignedEnd = ref(false);
const trailingPathCrumbs = ref<PathCrumb[]>([]);
let pathResizeObserver: ResizeObserver | null = null;

const rootPath = computed(() => {
    const protocol =
        selectedConnectionConfig.value?.protocol?.trim().toLowerCase() ?? "webdav";
    return protocol === "http-dlna" || protocol === "dlna" ? "0" : "/";
});

const collapsedPathCrumbs = ref<PathCrumb[]>([]);

const updatePathMenuAlignment = async () => {
    await nextTick();
    const pathBar = pathBarEl.value;
    const pathMenu = pathMenuEl.value;
    const pathOverflow = pathOverflowEl.value;
    if (!pathBar || !pathMenu || !pathOverflow) {
        isPathMenuAlignedEnd.value = false;
        return;
    }
    const pathBarRect = pathBar.getBoundingClientRect();
    const overflowRect = pathOverflow.getBoundingClientRect();
    const viewportRight = document.documentElement.clientWidth;
    const boundaryRight = Math.min(pathBarRect.right, viewportRight) - 8;
    isPathMenuAlignedEnd.value =
        overflowRect.left + pathMenu.offsetWidth > boundaryRight;
};

const togglePathOverflowMenu = async () => {
    if (isLoading.value || !collapsedPathCrumbs.value.length) return;
    isPathOverflowMenuOpen.value = !isPathOverflowMenuOpen.value;
    if (isPathOverflowMenuOpen.value) {
        await updatePathMenuAlignment();
    } else {
        isPathMenuAlignedEnd.value = false;
    }
};

const updatePathCrumbCollapse = async () => {
    await nextTick();
    const pathBar = pathBarEl.value;
    const pathMeasure = pathMeasureEl.value;
    const crumbs = pathCrumbs.value;
    if (!pathBar || !pathMeasure) {
        shouldCollapsePathCrumbs.value = false;
        leadingPathCrumbs.value = crumbs;
        trailingPathCrumbs.value = [];
        collapsedPathCrumbs.value = [];
        return;
    }
    const availableWidth = pathBar.clientWidth;
    if (pathMeasure.scrollWidth <= availableWidth + 1) {
        shouldCollapsePathCrumbs.value = false;
        leadingPathCrumbs.value = crumbs;
        trailingPathCrumbs.value = [];
        collapsedPathCrumbs.value = [];
        return;
    }

    const gapWidth = 4;
    const backWidth =
        pathMeasure.querySelector<HTMLElement>("[data-path-measure-back]")
            ?.offsetWidth ?? 0;
    const rootWidth =
        pathMeasure.querySelector<HTMLElement>("[data-path-measure-root]")
            ?.offsetWidth ?? 0;
    const sepWidth =
        pathMeasure.querySelector<HTMLElement>("[data-path-measure-sep]")
            ?.offsetWidth ?? 0;
    const crumbWidths = Array.from(
        pathMeasure.querySelectorAll<HTMLElement>("[data-path-measure-crumb]"),
    ).map((element) => element.offsetWidth);
    const ellipsisWidth = 24;
    const fixedChildren = [backWidth, rootWidth].filter((width) => width > 0);

    const getCandidateWidth = (indices: number[]) => {
        const sorted = [...indices].sort((a, b) => a - b);
        const trailingStart = Math.max(0, crumbs.length - 1);
        const leadingIndices = sorted.filter((value) => value < trailingStart);
        const trailingIndices = sorted.filter((value) => value >= trailingStart);
        const widths = [...fixedChildren];
        for (const index of leadingIndices) {
            if (index > 0) widths.push(sepWidth);
            widths.push(crumbWidths[index] ?? 0);
        }
        if (leadingIndices.length > 0) {
            widths.push(sepWidth);
        }
        widths.push(ellipsisWidth);
        for (const index of trailingIndices) {
            widths.push(sepWidth);
            const crumbWidth = crumbWidths[index] ?? 0;
            widths.push(index === crumbs.length - 1 ? Math.min(crumbWidth, 96) : crumbWidth);
        }
        return widths.reduce((total, width) => total + width, 0) +
            Math.max(0, widths.length - 1) * gapWidth;
    };

    const visible = new Set<number>();
    const canAdd = (index: number) =>
        getCandidateWidth([...visible, index]) <= availableWidth + 1;
    const addIfFits = (index: number) => {
        if (index < 0 || index >= crumbs.length || visible.has(index)) return false;
        if (!canAdd(index)) return false;
        visible.add(index);
        return true;
    };

    if (crumbs.length > 0) {
        visible.add(crumbs.length - 1);
    }
    for (let index = 0; index < crumbs.length - 1; index += 1) {
        addIfFits(index);
    }

    const visibleIndices = [...visible].sort((a, b) => a - b);
    const hiddenIndices = crumbs
        .map((_, index) => index)
        .filter((index) => !visible.has(index));
    const firstHiddenIndex = hiddenIndices[0] ?? crumbs.length;
    const lastHiddenIndex = hiddenIndices[hiddenIndices.length - 1] ?? -1;
    shouldCollapsePathCrumbs.value = true;
    leadingPathCrumbs.value = visibleIndices
        .filter((index) => index < firstHiddenIndex)
        .map((index) => crumbs[index]);
    trailingPathCrumbs.value = visibleIndices
        .filter((index) => index > lastHiddenIndex)
        .map((index) => crumbs[index]);
    collapsedPathCrumbs.value = hiddenIndices.map((index) => crumbs[index]);
    if (!collapsedPathCrumbs.value.length) {
        shouldCollapsePathCrumbs.value = false;
        leadingPathCrumbs.value = crumbs;
        trailingPathCrumbs.value = [];
    }
};

const onDocumentPointerDown = (event: PointerEvent) => {
    if (!isPathOverflowMenuOpen.value) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (pathOverflowEl.value?.contains(target)) return;
    isPathOverflowMenuOpen.value = false;
    isPathMenuAlignedEnd.value = false;
};

watch(
    [networkPath, viewMode, pathCrumbs],
    () => {
        isPathOverflowMenuOpen.value = false;
        void updatePathCrumbCollapse();
    },
    { deep: true },
);

onMounted(() => {
    document.addEventListener("pointerdown", onDocumentPointerDown, true);
    if (typeof ResizeObserver !== "undefined") {
        pathResizeObserver = new ResizeObserver(() => {
            void updatePathCrumbCollapse();
            if (isPathOverflowMenuOpen.value) {
                void updatePathMenuAlignment();
            }
        });
        if (pathBarEl.value) pathResizeObserver.observe(pathBarEl.value);
    }
    void updatePathCrumbCollapse();
    void updatePathMenuAlignment();
});

onUnmounted(() => {
    document.removeEventListener("pointerdown", onDocumentPointerDown, true);
    pathResizeObserver?.disconnect();
    pathResizeObserver = null;
});

watch(
    pathBarEl,
    (element, previousElement) => {
        if (!pathResizeObserver) return;
        if (previousElement) pathResizeObserver.unobserve(previousElement);
        if (element) pathResizeObserver.observe(element);
    },
);

const toErrorMessage = (error: unknown, fallback: string) => {
    if (error instanceof Error && error.message) return error.message;
    if (typeof error === "string" && error) return error;
    return fallback;
};

const isSmbLogonFailure = (error: unknown) => {
    const message = toErrorMessage(error, "").toLowerCase();
    return (
        message.includes("status_logon_failure") ||
        message.includes("0xc000006d") ||
        message.includes("logon_failure")
    );
};

const maybeOpenSmbCredentialsEditor = (error: unknown) => {
    if (!isSmbLogonFailure(error)) return false;
    const connection = selectedConnectionConfig.value;
    if (!connection) return false;
    const protocol = connection.protocol.trim().toLowerCase();
    if (protocol !== "smb" && protocol !== "samba") return false;
    if (connection.username.trim() || connection.password) return false;
    openEditModal(connection);
    return true;
};

const hasExplicitScheme = (value: string) => /^[a-z][a-z0-9+.-]*:\/\//i.test(value);

// `host:port/path` typed without a scheme is either rejected by URL parsing or read as an
// opaque URL whose scheme is the host name, which drops the port. Add the default scheme
// here so the saved URL matches what the backend browses.
const normalizeHttpBaseUrl = (value: string) => {
    const trimmed = value.trim();
    if (!trimmed || hasExplicitScheme(trimmed)) return trimmed;
    return `http://${trimmed}`;
};

const buildConnectionBaseUrl = () => {
    if (isSmbProtocol.value) {
        const host = createForm.host.trim();
        const share = createForm.share.trim().replace(/^\/+/, "");
        if (!host) {
            throw new Error("SMB host is required");
        }
        return share ? `smb://${host}/${share}` : `smb://${host}/`;
    }
    if (isFtpProtocol.value) {
        const host = createForm.host.trim();
        const port = createForm.port.trim() || "21";
        if (!host) {
            throw new Error("FTP host is required");
        }
        return `ftp://${host}:${port}`;
    }
    const baseUrl = normalizeHttpBaseUrl(createForm.baseUrl);
    if (!baseUrl) {
        throw new Error(
            isHttpDlnaProtocol.value
                ? "DLNA device URL is required"
                : "Server URL is required",
        );
    }
    return baseUrl;
};

const buildAutoConnectionLabel = (baseUrl: string) => {
    if (isSmbProtocol.value) {
        const host = createForm.host.trim();
        const share = createForm.share.trim().replace(/^\/+/, "");
        if (host && share) return `${share} @ ${host}`;
        if (host) return host;
        return "Connection";
    }
    if (isFtpProtocol.value) {
        const host = createForm.host.trim();
        return host || "Connection";
    }
    try {
        const parsed = new URL(baseUrl);
        return parsed.hostname || "Connection";
    } catch {
        return "Connection";
    }
};

const onCreateConnectionSubmit = async () => {
    const label = createForm.label.trim();
    const username =
        requiresAuthFields.value && isSmbProtocol.value && createForm.group.trim()
            ? `${createForm.group.trim()};${createForm.username.trim()}`
            : requiresAuthFields.value
              ? createForm.username.trim()
              : "";
    const defaultPath = createForm.defaultPath.trim() || "/";
    const isEditing = Boolean(editingConnectionId.value);

    isCreatingConnection.value = true;
    createError.value = "";

    try {
        const baseUrl = buildConnectionBaseUrl();
        const normalizedLabel = label || buildAutoConnectionLabel(baseUrl);
        const shouldRefreshBrowserAfterSave =
            isEditing && viewMode.value === "browser";
        if (isEditing) {
            selectedConnection.value = editingConnectionId.value ?? "";
        } else {
            onCreateConnection(createForm.protocol);
        }
        const connection = selectedConnectionConfig.value as
            | NetworkConnection
            | null;
        if (!connection) {
            throw new Error("Connection draft is not ready");
        }
        connection.label = normalizedLabel;
        connection.protocol = createForm.protocol;
        if (connection.baseUrl !== baseUrl) {
            connection.tlsCertificateDer = null;
        }
        connection.baseUrl = baseUrl;
        connection.username = username;
        connection.password = requiresAuthFields.value ? createForm.password : "";
        connection.defaultPath = defaultPath;
        await onSaveConnection();
        isCreateModalOpen.value = false;
        editingConnectionId.value = null;
        resetCreateForm();
        if (shouldRefreshBrowserAfterSave) {
            await onRefresh();
        }
    } catch (error) {
        createError.value = toErrorMessage(
            error,
            isEditing ? "Failed to save connection" : "Failed to create connection",
        );
    } finally {
        isCreatingConnection.value = false;
    }
};

const onDeleteConnectionItem = async (connection: NetworkConnection) => {
    pendingDeleteConnection.value = connection;
    deleteError.value = "";
    isDeleteModalOpen.value = true;
};

const onDeleteConnectionConfirm = async () => {
    const connection = pendingDeleteConnection.value;
    if (!connection) return;
    isDeletingConnection.value = true;
    deleteError.value = "";
    try {
        selectedConnection.value = connection.id;
        await onDeleteConnection();
        const stillExists = networkConnections.value.some(
            (item) => item.id === connection.id,
        );
        if (stillExists) {
            deleteError.value = errorMessage.value || "Failed to delete connection";
            return;
        }
        closeDeleteModal(true);
    } finally {
        isDeletingConnection.value = false;
    }
};

const titleStatusText = computed(() =>
    isLoading.value ? "Loading" : connectionStatus.value,
);

const shouldShowTitleStatus = computed(
    () => titleStatusText.value !== "Connected",
);

const isEditingConnection = computed(() => Boolean(editingConnectionId.value));
const pendingDeleteLabel = computed(
    () =>
        pendingDeleteConnection.value?.label ||
        pendingDeleteConnection.value?.id ||
        tr("this connection"),
);
const deleteConfirmMessage = computed(
    () => `${tr("Delete connection")} "${pendingDeleteLabel.value}"?`,
);

const formatProtocolLabel = (protocol: string) =>
    protocol?.trim() ? protocol.trim().toUpperCase() : "WEBDAV";
</script>

<template>
    <div class="panel panel--network">
        <div
            class="network-body"
        >
            <div class="panel__header network-header">
            <div class="network-header__main">
                <div
                    class="panel__title network-title"
                    :class="{ 'network-title--browser': viewMode === 'browser' }"
                >
                    <template v-if="viewMode === 'connections'">
                        {{ tr("Network") }}
                    </template>
                    <template v-else>
                        <button
                            class="network-icon-btn network-icon-btn--close network-title__home-btn"
                            type="button"
                            :aria-label="tr('Home')"
                            :title="tr('Home')"
                            @click="onBackToConnections"
                        >
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                viewBox="0 -960 960 960"
                                fill="#e3e3e3"
                            >
                                <path
                                    d="M240-200h120v-240h240v240h120v-360L480-740 240-560v360Zm-80 80v-480l320-240 320 240v480H520v-240h-80v240H160Zm320-350Z"
                                />
                            </svg>
                        </button>
                        <span class="network-title__label">
                            {{ activeConnectionLabel }}
                        </span>
                        <span
                            v-if="shouldShowTitleStatus"
                            class="network-title__status"
                        >
                            {{ tr(titleStatusText) }}
                        </span>
                        <span
                            v-if="errorMessage"
                            class="network-title__error"
                            :title="errorMessage"
                        >
                            {{ errorMessage }}
                        </span>
                    </template>
                </div>
                <div v-if="viewMode === 'connections'" class="network-header__meta">
                    {{ `${networkConnections.length} ${tr("connections")}` }}
                </div>
                <div
                    v-else-if="shouldShowPathBar"
                    ref="pathBarEl"
                    class="network-title__path"
                    :class="{
                        'network-title__path--collapsed': shouldCollapsePathCrumbs,
                    }"
                    :title="networkPath"
                >
                    <button
                        v-if="parentPath"
                        class="network-title__back-btn"
                        type="button"
                        :disabled="isLoading"
                        :aria-label="tr('Go to parent folder')"
                        @click="onBackFolderClick"
                    >
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            viewBox="0 -960 960 960"
                            fill="#e3e3e3"
                        >
                            <path
                                d="m313-440 224 224-57 56-320-320 320-320 57 56-224 224h487v80H313Z"
                            />
                        </svg>
                    </button>
                    <button
                        class="network-title__path-crumb network-title__path-root"
                        type="button"
                        :disabled="isLoading || networkPath === rootPath"
                        title="/"
                        @click="onPathCrumbClick(rootPath)"
                    >
                        /
                    </button>
                    <template
                        v-for="(crumb, index) in leadingPathCrumbs"
                        :key="crumb.path"
                    >
                        <span
                            v-if="index > 0"
                            class="network-title__path-sep"
                        >
                            /
                        </span>
                        <button
                            class="network-title__path-crumb"
                            :class="{
                                'network-title__path-crumb--current':
                                    crumb.path === networkPath,
                            }"
                            type="button"
                            :title="crumb.label"
                            :disabled="isLoading || crumb.path === networkPath"
                            @click="onPathCrumbClick(crumb.path)"
                        >
                            {{ crumb.label }}
                        </button>
                    </template>
                    <template v-if="shouldCollapsePathCrumbs">
                        <span
                            v-if="leadingPathCrumbs.length > 0"
                            class="network-title__path-sep"
                        >
                            /
                        </span>
                        <div
                            ref="pathOverflowEl"
                            class="network-title__path-overflow"
                        >
                            <button
                                class="network-title__path-crumb network-title__path-ellipsis"
                                type="button"
                                :disabled="isLoading"
                                :aria-label="tr('Show hidden path folders')"
                                :aria-expanded="isPathOverflowMenuOpen"
                                @click="togglePathOverflowMenu"
                            >
                                ...
                            </button>
                            <div
                                v-if="isPathOverflowMenuOpen"
                                ref="pathMenuEl"
                                class="network-title__path-menu"
                                :class="{
                                    'network-title__path-menu--align-end':
                                        isPathMenuAlignedEnd,
                                }"
                            >
                                <button
                                    v-for="crumb in collapsedPathCrumbs"
                                    :key="crumb.path"
                                    class="network-title__path-menu-item"
                                    type="button"
                                    :title="crumb.label"
                                    :disabled="isLoading || crumb.path === networkPath"
                                    @click="onPathCrumbClick(crumb.path)"
                                >
                                    {{ crumb.label }}
                                </button>
                            </div>
                        </div>
                    </template>
                    <template
                        v-for="crumb in trailingPathCrumbs"
                        :key="crumb.path"
                    >
                        <span class="network-title__path-sep">
                            /
                        </span>
                        <button
                            class="network-title__path-crumb"
                            :class="{
                                'network-title__path-crumb--current':
                                    crumb.path === networkPath,
                            }"
                            type="button"
                            :title="crumb.label"
                            :disabled="isLoading || crumb.path === networkPath"
                            @click="onPathCrumbClick(crumb.path)"
                        >
                            {{ crumb.label }}
                        </button>
                    </template>
                </div>
                <div
                    v-if="shouldShowPathBar"
                    ref="pathMeasureEl"
                    class="network-title__path network-title__path-measure"
                    aria-hidden="true"
                >
                    <button
                        v-if="parentPath"
                        class="network-title__back-btn"
                        type="button"
                        tabindex="-1"
                        data-path-measure-back
                    >
                        <svg viewBox="0 -960 960 960">
                            <path
                                d="m313-440 224 224-57 56-320-320 320-320 57 56-224 224h487v80H313Z"
                            />
                        </svg>
                    </button>
                    <button
                        class="network-title__path-crumb network-title__path-root"
                        type="button"
                        tabindex="-1"
                        data-path-measure-root
                    >
                        /
                    </button>
                    <template
                        v-for="(crumb, index) in pathCrumbs"
                        :key="`measure-${crumb.path}`"
                    >
                        <span
                            v-if="index > 0"
                            class="network-title__path-sep"
                            data-path-measure-sep
                        >
                            /
                        </span>
                        <button
                            class="network-title__path-crumb"
                            type="button"
                            tabindex="-1"
                            data-path-measure-crumb
                        >
                            {{ crumb.label }}
                        </button>
                    </template>
                </div>
            </div>
            <div class="network-header__actions">
                <template v-if="viewMode === 'connections'">
                    <button
                        v-if="canDiscoverConnections"
                        class="network-new-btn network-new-btn--refresh"
                        :class="{ 'network-new-btn--spinning': isDiscovering }"
                        type="button"
                        :aria-label="tr('Refresh connections')"
                        :title="isDiscovering ? tr('Refreshing') : tr('Refresh')"
                        :disabled="isDiscovering"
                        @click="onConnectionsRefreshClick"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <path d="M21 12a9 9 0 1 1-2.64-6.36" />
                            <path d="M21 3v6h-6" />
                        </svg>
                    </button>
                    <button
                        class="network-new-btn"
                        type="button"
                        :aria-label="tr('Add connection')"
                        :title="tr('New')"
                        @click="openCreateModal"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                        >
                            <path
                                d="M12 5v14M5 12h14"
                                stroke-width="2"
                                stroke-linecap="round"
                            />
                        </svg>
                    </button>
                </template>
                <template v-else>
                    <label class="network-sort-control">
                        <CustomSelect
                            v-model="networkSortValue"
                            :options="networkSortOptions"
                            :aria-label="tr('Sort network entries')"
                        />
                    </label>
                    <button
                        class="network-icon-btn"
                        type="button"
                        :aria-label="tr('Refresh')"
                        :title="tr('Refresh')"
                        @click="onRefreshClick"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <path d="M21 12a9 9 0 1 1-2.64-6.36" />
                            <path d="M21 3v6h-6" />
                        </svg>
                    </button>
                </template>
            </div>
        </div>

        <transition name="network-view-switch" mode="out-in">
            <NetworkConnectionsView
                v-if="viewMode === 'connections'"
                key="connections"
                :network-connections="networkConnections"
                :selected-connection="selectedConnection"
                :format-protocol-label="formatProtocolLabel"
                @open-browser="onOpenConnectionBrowser"
                @edit="openEditModal"
                @delete="onDeleteConnectionItem"
            />
            <NetworkBrowserView
                v-else
                key="browser"
                :is-loading="isLoading"
                :network-path="networkPath"
                :entries="visibleNetworkEntries"
                :has-files="hasFiles"
                @entry-click="onEntryClick"
            />
        </transition>

        <transition name="network-switch-fade">
            <div
                v-if="isSwitchingView && viewMode === 'connections'"
                class="network-switch-indicator"
                aria-live="polite"
            >
                <div class="network-switch-spinner"></div>
                <div class="network-switch-text">{{ tr("Opening connection...") }}</div>
            </div>
        </transition>
        </div>

        <NetworkConnectionModal
            :open="isCreateModalOpen"
            :is-editing-connection="isEditingConnection"
            :selected-protocol-label="selectedProtocolLabel"
            :protocol-options="protocolOptions"
            :create-form="createForm"
            :is-smb-protocol="isSmbProtocol"
            :is-ftp-protocol="isFtpProtocol"
            :requires-auth-fields="requiresAuthFields"
            :server-field-label="serverFieldLabel"
            :server-field-placeholder="serverFieldPlaceholder"
            :default-path-label="defaultPathLabel"
            :create-error="tr(createError)"
            :is-creating-connection="isCreatingConnection"
            @close="closeCreateModal"
            @submit="onCreateConnectionSubmit"
        />
        <ConfirmDialog
            :open="isDeleteModalOpen"
            :title="tr('Delete Connection')"
            :message="deleteConfirmMessage"
            :confirm-text="isDeletingConnection ? tr('Deleting...') : tr('Delete')"
            :confirm-loading="isDeletingConnection"
            :error-message="tr(deleteError)"
            @cancel="closeDeleteModal"
            @confirm="onDeleteConnectionConfirm"
        />
    </div>
</template>

<style src="../styles/panels.css"></style>
<style src="../styles/network-panel.css"></style>
