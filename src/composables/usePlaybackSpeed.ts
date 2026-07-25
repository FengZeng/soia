import { ref } from "vue";
import type { CoreClient } from "../core-client/CoreClient";

export const usePlaybackSpeed = (coreClient: CoreClient) => {
    const playbackRates = [2.0, 1.75, 1.5, 1.25, 1.0, 0.75, 0.5, 0.25];
    const currentSpeed = ref(1.0);
    const showSpeedMenu = ref(false);

    const setSpeed = async (rate: number) => {
        currentSpeed.value = rate;
        showSpeedMenu.value = false;
        await coreClient.execute({ type: "setSpeed", speed: rate });
    };

    return {
        playbackRates,
        currentSpeed,
        showSpeedMenu,
        setSpeed,
    };
};
