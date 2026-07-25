import { computed, ref } from "vue";

export type ContextMenuItem = {
    id: string;
    label: string;
    icon?: "heart" | "settings" | "subtitle" | "subtitle-advanced-settings" | "subtitle-search" | "remote-control";
    disabled?: boolean;
    children?: ContextMenuItem[];
};

type UsePlaybackContextMenuOptions = {
    isFileLoaded: () => boolean;
    getCurrentPath: () => string;
    getCurrentTitle: () => string;
    addToFavorites: (item: { path: string; title?: string }) => void;
    searchOnlineSubtitles: (path: string, title?: string) => void | Promise<void>;
    openSubtitleAdvancedSettings: () => void;
    openSettings: () => void | Promise<void>;
    isRemoteControlEnabled: () => Promise<boolean>;
    showRemoteControlQr: () => void | Promise<void>;
    hideAllMenus: () => void;
};

const ADD_TO_FAVORITES_ID = "add-to-favorites";
const SUBTITLE_ID = "subtitle";
const SUBTITLE_FIND_ONLINE_ID = "subtitle/find-online-subtitle";
const SUBTITLE_ADVANCED_SETTINGS_ID = "subtitle/advanced-settings";
const OPEN_SETTINGS_ID = "open-settings";
const REMOTE_CONTROL_QR_CODE_ID = "remote-control-qr-code";

const isInteractiveContextTarget = (target: HTMLElement | null) =>
    !!target?.closest(
        [
            "a",
            "button",
            "input",
            "textarea",
            "select",
            "[contenteditable='true']",
            "[role='menu']",
            "[role='menuitem']",
            ".top-bar",
            ".player-controls-content",
            ".playlist-drawer",
            ".side-actions-nav",
            ".track-menu-container",
        ].join(", "),
    );

export const usePlaybackContextMenu = ({
    isFileLoaded,
    getCurrentPath,
    getCurrentTitle,
    addToFavorites,
    searchOnlineSubtitles,
    openSubtitleAdvancedSettings,
    openSettings,
    isRemoteControlEnabled,
    showRemoteControlQr,
    hideAllMenus,
}: UsePlaybackContextMenuOptions) => {
    const isOpen = ref(false);
    const position = ref({ x: 0, y: 0 });
    const remoteControlEnabled = ref(false);
    let contextMenuRequestId = 0;

    const items = computed<ContextMenuItem[]>(() => {
        const hasPath = !!getCurrentPath().trim();
        const menuItems: ContextMenuItem[] = [
            {
                id: ADD_TO_FAVORITES_ID,
                label: "Add to Favorites",
                icon: "heart",
                disabled: !hasPath,
            },
            {
                id: SUBTITLE_ID,
                label: "Subtitle",
                icon: "subtitle",
                disabled: !hasPath,
                children: [
                    {
                        id: SUBTITLE_ADVANCED_SETTINGS_ID,
                        label: "Advanced Subtitle Settings",
                        icon: "subtitle-advanced-settings",
                        disabled: !hasPath,
                    },
                    {
                        id: SUBTITLE_FIND_ONLINE_ID,
                        label: "Find Online Subtitles",
                        icon: "subtitle-search",
                        disabled: !hasPath,
                    },
                ],
            },
            {
                id: OPEN_SETTINGS_ID,
                label: "Open Settings",
                icon: "settings",
            },
        ];
        if (remoteControlEnabled.value) {
            menuItems.push({
                id: REMOTE_CONTROL_QR_CODE_ID,
                label: "Remote Control",
                icon: "remote-control",
            });
        }
        return menuItems;
    });

    const close = () => {
        contextMenuRequestId += 1;
        isOpen.value = false;
    };

    const openAt = (x: number, y: number) => {
        position.value = { x, y };
        isOpen.value = true;
    };

    const onContextMenu = async (event: MouseEvent) => {
        if (!isFileLoaded()) return;
        if (isInteractiveContextTarget(event.target as HTMLElement | null)) return;

        const path = getCurrentPath().trim();
        if (!path) return;

        event.preventDefault();
        event.stopPropagation();
        hideAllMenus();
        const requestId = ++contextMenuRequestId;
        const enabled = await isRemoteControlEnabled().catch(() => false);
        if (requestId !== contextMenuRequestId) return;
        remoteControlEnabled.value = enabled;
        openAt(event.clientX, event.clientY);
    };

    const onSelect = (id: string) => {
        if (id === ADD_TO_FAVORITES_ID) {
            const path = getCurrentPath().trim();
            if (path) {
                addToFavorites({
                    path,
                    title: getCurrentTitle().trim() || undefined,
                });
            }
        }
        if (id === SUBTITLE_FIND_ONLINE_ID) {
            const path = getCurrentPath().trim();
            if (path) {
                void searchOnlineSubtitles(path, getCurrentTitle().trim() || undefined);
            }
        }
        if (id === SUBTITLE_ADVANCED_SETTINGS_ID) {
            openSubtitleAdvancedSettings();
        }
        if (id === REMOTE_CONTROL_QR_CODE_ID) {
            void showRemoteControlQr();
        }
        close();
        if (id === OPEN_SETTINGS_ID) {
            void openSettings();
        }
    };

    return {
        isOpen,
        position,
        items,
        onContextMenu,
        onSelect,
        close,
    };
};
