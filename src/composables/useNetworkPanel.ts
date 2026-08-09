import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { NetworkFileRow, NetworkPlayRequest } from "../types/network";
import {
    NETWORK_START_AT_ROOT_SETTING_LABEL,
    SETTINGS_UPDATED_EVENT,
} from "../mock/settings";
import { useNetworkConnections } from "./useNetworkConnections";
import { useNetworkBrowser } from "./useNetworkBrowser";
import {
    createDebouncedUiStateSaver,
    loadUiState as loadUiStateStore,
} from "./useUiStateStore";

type StoredNetworkState = {
    selectedConnection?: string;
    path?: string;
};

type StoredSettingGroups = Array<{
    title: string;
    items: Array<{ label: string; value: string }>;
}>;

type StoredSettingsState = {
    settings?: {
        groups?: StoredSettingGroups;
    };
};

type SettingsUpdatedDetail = {
    groups?: StoredSettingGroups;
};

type NetworkPanelViewMode = "connections" | "browser";

const isSettingEnabled = (
    groups: StoredSettingGroups | undefined,
    label: string,
): boolean =>
    groups
        ?.flatMap((group) => group.items)
        .find((item) => item.label === label)
        ?.value === "On";

export const useNetworkPanel = () => {
    const {
        connections,
        selectedConnectionId,
        selectedConnection,
        loadConnections,
        createConnection,
        saveSelectedConnection,
        deleteSelectedConnection,
        discoverConnections,
    } = useNetworkConnections();
    const browser = useNetworkBrowser(selectedConnectionId, selectedConnection);
    const viewMode = ref<NetworkPanelViewMode>("connections");
    const hasConnected = ref(false);
    const shouldStartAtRoot = ref(false);
    const activeConnectionLabel = computed(
        () => selectedConnection.value?.label || "Network",
    );
    const connectionStatus = computed(() =>
        browser.errorMessage.value
            ? "Error"
            : hasConnected.value
              ? "Connected"
              : "Idle",
    );

    const uiStateSaver = createDebouncedUiStateSaver(300);

    const toErrorMessage = (error: unknown, fallback: string) => {
        if (error instanceof Error && error.message) return error.message;
        if (typeof error === "string" && error) return error;
        return fallback;
    };

    const saveUiState = () => {
        const protocol = selectedConnection.value?.protocol?.toLowerCase() ?? "webdav";
        const isDlna = protocol === "http-dlna" || protocol === "dlna";
        uiStateSaver.saveDebounced({
            network: {
                selectedConnection: selectedConnectionId.value,
                path: isDlna ? "0" : browser.networkPath.value,
            },
        });
    };

    const getRootPathForSelectedConnection = () => {
        const protocol = selectedConnection.value?.protocol?.toLowerCase() ?? "webdav";
        return protocol === "http-dlna" || protocol === "dlna" ? "0" : "/";
    };

    const applySettingsGroups = (groups?: StoredSettingGroups) => {
        shouldStartAtRoot.value = isSettingEnabled(
            groups,
            NETWORK_START_AT_ROOT_SETTING_LABEL,
        );
    };

    const loadStoredUiState = async () => {
        const stored = await loadUiStateStore<
            { network?: StoredNetworkState } & StoredSettingsState
        >();
        applySettingsGroups(stored?.settings?.groups);
        const network = stored?.network;
        if (network?.selectedConnection) {
            selectedConnectionId.value = network.selectedConnection;
        }
        if (network?.path) {
            browser.networkPath.value = shouldStartAtRoot.value
                ? getRootPathForSelectedConnection()
                : selectedConnection.value?.protocol === "http-dlna"
                  ? "0"
                  : browser.normalizePath(network.path);
        }
    };

    const onSettingsUpdated = (event: Event) => {
        const customEvent = event as CustomEvent<SettingsUpdatedDetail>;
        applySettingsGroups(customEvent.detail?.groups);
    };

    const onConnect = async () => {
        try {
            await saveSelectedConnection();
            await browser.connect();
            hasConnected.value = true;
        } catch (error) {
            hasConnected.value = false;
            browser.errorMessage.value = toErrorMessage(
                error,
                "Failed to connect network server",
            );
        }
    };

    const onOpenBrowser = async (connectionId?: string) => {
        if (connectionId) {
            selectedConnectionId.value = connectionId;
        }
        const connection = selectedConnection.value;
        if (!connection) return;
        browser.networkPath.value = shouldStartAtRoot.value
            ? getRootPathForSelectedConnection()
            : connection.protocol === "http-dlna"
              ? "0"
              : browser.normalizePath(connection.defaultPath || "/");
        viewMode.value = "browser";
        await onConnect();
    };

    const resetBrowserToRootWhenVisible = async () => {
        if (!shouldStartAtRoot.value) return;
        if (viewMode.value !== "browser") return;
        const rootPath = getRootPathForSelectedConnection();
        if (!hasConnected.value) {
            browser.networkPath.value = rootPath;
            return;
        }
        if (browser.networkPath.value === rootPath && browser.currentFiles.value.length) {
            return;
        }
        try {
            await openDirectory(rootPath, { rememberPath: false });
            hasConnected.value = true;
        } catch (error) {
            browser.errorMessage.value = toErrorMessage(
                error,
                "Failed to open network root",
            );
        }
    };

    const onBackToConnections = () => {
        viewMode.value = "connections";
    };

    const onRefresh = async () => {
        try {
            await browser.refresh();
            hasConnected.value = true;
        } catch (error) {
            browser.errorMessage.value = toErrorMessage(
                error,
                "Failed to refresh directory",
            );
        }
    };

    const onSaveConnection = async () => {
        try {
            await saveSelectedConnection();
            browser.errorMessage.value = "";
        } catch (error) {
            browser.errorMessage.value = toErrorMessage(
                error,
                "Failed to save connection",
            );
        }
    };

    const onCreateConnection = (protocol = "webdav") => {
        createConnection(protocol);
        viewMode.value = "connections";
    };

    const onDeleteConnection = async () => {
        try {
            await deleteSelectedConnection();
            browser.currentFiles.value = [];
            browser.errorMessage.value = "";
            hasConnected.value = false;
            viewMode.value = "connections";
        } catch (error) {
            browser.errorMessage.value = toErrorMessage(
                error,
                "Failed to delete connection",
            );
        }
    };

    const onDiscoverConnections = async () => {
        await discoverConnections();
    };

    const openEntry = async (entry: NetworkFileRow) => {
        if (entry.type !== "DIR") return;
        await openDirectory(entry.path);
    };

    const onBrowsePath = async (path: string) => {
        await openDirectory(path);
    };

    const openDirectory = async (
        path: string,
        { rememberPath = true }: { rememberPath?: boolean } = {},
    ) => {
        await browser.openDirectory(path);
        if (!rememberPath) return;
        const connection = selectedConnection.value;
        if (connection) {
            connection.defaultPath = browser.networkPath.value;
        }
        // `defaultPath` is the shared last-opened directory for this connection.
        // Persist it after a successful browse so remote clients can resume here too.
        await saveSelectedConnection();
    };

    const buildPlayRequest = (entry: NetworkFileRow): NetworkPlayRequest => {
        if (!entry.playbackKey) {
            throw new Error("Network file is missing playback key");
        }
        return {
            playbackKey: entry.playbackKey,
            displayName: entry.name,
        };
    };

    watch([selectedConnectionId, browser.networkPath], saveUiState);

    watch(
        selectedConnection,
        (connection) => {
            if (!connection) return;
            if (!connection.defaultPath) {
                connection.defaultPath =
                    connection.protocol === "http-dlna" ? "0" : "/";
            }
        },
        { immediate: true },
    );

    watch(
        () => selectedConnectionId.value,
        () => {
            hasConnected.value = false;
        },
    );

    onMounted(async () => {
        if (typeof window !== "undefined") {
            window.addEventListener(SETTINGS_UPDATED_EVENT, onSettingsUpdated);
        }
        await loadConnections();
        await loadStoredUiState();
        try {
            await onDiscoverConnections();
        } catch {
            // Keep startup non-blocking even when discovery fails.
        }
        if (!selectedConnection.value && connections.value.length > 0) {
            selectedConnectionId.value = connections.value[0].id;
        }
    });

    onUnmounted(() => {
        if (typeof window === "undefined") return;
        window.removeEventListener(SETTINGS_UPDATED_EVENT, onSettingsUpdated);
    });

    return {
        viewMode,
        activeConnectionLabel,
        networkConnections: connections,
        selectedConnection: selectedConnectionId,
        selectedConnectionConfig: selectedConnection,
        networkEntries: browser.networkEntries,
        pathCrumbs: browser.pathCrumbs,
        parentPath: browser.parentPath,
        networkPath: browser.networkPath,
        isLoading: browser.isLoading,
        connectionStatus,
        errorMessage: browser.errorMessage,
        hasFiles: browser.hasFiles,
        openEntry,
        onBrowsePath,
        onConnect,
        onRefresh,
        onSaveConnection,
        onCreateConnection,
        onDeleteConnection,
        onDiscoverConnections,
        onOpenBrowser,
        resetBrowserToRootWhenVisible,
        onBackToConnections,
        buildPlayRequest,
        getAncestorPaths: browser.getAncestorPaths,
    };
};
