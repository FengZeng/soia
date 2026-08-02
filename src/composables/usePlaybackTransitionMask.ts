import { nextTick, readonly, ref } from "vue";

const MASK_PAINT_TIMEOUT_MS = 100;

const waitForMaskPaint = () =>
    new Promise<void>((resolve) => {
        let settled = false;
        const finish = () => {
            if (settled) return;
            settled = true;
            window.clearTimeout(timeoutId);
            resolve();
        };
        const timeoutId = window.setTimeout(finish, MASK_PAINT_TIMEOUT_MS);

        // The second callback runs after the browser has had an opportunity to
        // paint the mask. The timeout prevents a hidden/throttled WebView from
        // blocking playback indefinitely.
        window.requestAnimationFrame(() => window.requestAnimationFrame(finish));
    });

/**
 * Covers native video surfaces while MPV changes files.
 *
 * MPV's source-loading state ends when it accepts a load command, which can be
 * earlier than the first presented frame. This mask instead spans playback
 * intent/EOF through MPV_EVENT_PLAYBACK_RESTART.
 */
export const usePlaybackTransitionMask = () => {
    const isVisible = ref(false);
    let generation = 0;

    const activate = () => {
        // Invalidate a delayed release belonging to an older transition.
        generation += 1;
        isVisible.value = true;
    };

    const clear = () => {
        generation += 1;
        isVisible.value = false;
    };

    const activateAndWaitForPaint = async () => {
        activate();
        await nextTick();
        await waitForMaskPaint();
    };

    const releaseAfterPlaybackRestart = async () => {
        const releaseGeneration = generation;
        await waitForMaskPaint();
        if (releaseGeneration !== generation) return;
        isVisible.value = false;
    };

    return {
        isVisible: readonly(isVisible),
        activate,
        clear,
        activateAndWaitForPaint,
        releaseAfterPlaybackRestart,
    };
};
