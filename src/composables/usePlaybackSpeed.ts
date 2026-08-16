import { ref } from "vue";
import type { CoreClient } from "../core-client/CoreClient";

export const usePlaybackSpeed = (coreClient: CoreClient) => {
    const playbackRates = [3.0, 2.0, 1.5, 1.25, 1.0, 0.75, 0.5, 0.25];
    const currentSpeed = ref(1.0);
    const showSpeedMenu = ref(false);
    let pendingContinuousSpeed: number | null = null;
    let isApplyingContinuousSpeed = false;

    const setSpeed = async (rate: number, closeMenu = true) => {
        currentSpeed.value = rate;
        if (closeMenu) showSpeedMenu.value = false;
        await coreClient.execute({ type: "setSpeed", speed: rate });
    };

    const applyPendingContinuousSpeed = async () => {
        if (isApplyingContinuousSpeed) return;
        isApplyingContinuousSpeed = true;
        try {
            while (pendingContinuousSpeed != null) {
                const rate = pendingContinuousSpeed;
                pendingContinuousSpeed = null;
                try {
                    await coreClient.execute({ type: "setSpeed", speed: rate });
                } catch (error) {
                    console.warn("[playbackSpeed] Failed to set playback speed", error);
                }
            }
        } finally {
            isApplyingContinuousSpeed = false;
        }
    };

    const setSpeedContinuously = (rate: number) => {
        currentSpeed.value = rate;
        pendingContinuousSpeed = rate;
        void applyPendingContinuousSpeed();
    };

    return {
        playbackRates,
        currentSpeed,
        showSpeedMenu,
        setSpeed,
        setSpeedContinuously,
    };
};
