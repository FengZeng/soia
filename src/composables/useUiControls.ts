import { ref } from "vue";

const MANUAL_MOUSE_REVEAL_SUPPRESSION_MS = 3000;

export const useUiControls = (
    isFileLoaded: () => boolean,
    onHideMenus: () => void,
    shouldKeepVisible: () => boolean,
    onShowControls?: () => void,
) => {
    const showControls = ref(true);
    const hoverFilePicker = ref(false);
    let hideTimeout: ReturnType<typeof setTimeout> | null = null;
    let suppressMouseRevealUntil = 0;

    const hideBar = (force = false) => {
        if (!force && shouldKeepVisible()) {
            resetInactivityTimer();
            return;
        }
        showControls.value = false;
        onHideMenus();
    };
    const showBar = () => {
        const wasHidden = !showControls.value;
        showControls.value = true;
        if (wasHidden) onShowControls?.();
    };

    const resetInactivityTimer = () => {
        if (hideTimeout) clearTimeout(hideTimeout);
        hideTimeout = setTimeout(hideBar, 2000);
    };

    const stopInactivityTimer = () => {
        if (hideTimeout) clearTimeout(hideTimeout);
        hideTimeout = null;
    };

    const onUserInteraction = () => {
        if (!isFileLoaded()) return;
        suppressMouseRevealUntil = 0;
        showBar();
        resetInactivityTimer();
    };

    const onMouseMove = () => {
        if (!isFileLoaded()) return;
        if (Date.now() < suppressMouseRevealUntil) return;
        suppressMouseRevealUntil = 0;
        showBar();
        resetInactivityTimer();
    };

    const toggleControlsFromMiddleClick = () => {
        if (!isFileLoaded()) return;
        if (showControls.value) {
            suppressMouseRevealUntil =
                Date.now() + MANUAL_MOUSE_REVEAL_SUPPRESSION_MS;
            stopInactivityTimer();
            hideBar(true);
            return;
        }

        suppressMouseRevealUntil = 0;
        showBar();
        resetInactivityTimer();
    };

    const cleanup = () => {
        stopInactivityTimer();
        suppressMouseRevealUntil = 0;
    };

    return {
        showControls,
        hoverFilePicker,
        onUserInteraction,
        onMouseMove,
        toggleControlsFromMiddleClick,
        resetInactivityTimer,
        stopInactivityTimer,
        cleanup,
    };
};
