import { onMounted, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type UseRemotePlaybackNavigationOptions = {
    playPreviousTrack: () => Promise<void>;
    playNextTrack: () => Promise<void>;
};

const REMOTE_PLAYBACK_NAVIGATION_EVENT = "soia-remote-playback-navigation";

export const useRemotePlaybackNavigation = ({
    playPreviousTrack,
    playNextTrack,
}: UseRemotePlaybackNavigationOptions) => {
    let unlisten: UnlistenFn | null = null;

    onMounted(async () => {
        unlisten = await listen<number>(
            REMOTE_PLAYBACK_NAVIGATION_EVENT,
            ({ payload }) => {
                if (payload === -1) {
                    void playPreviousTrack();
                } else if (payload === 1) {
                    void playNextTrack();
                }
            },
        );
    });

    onUnmounted(() => {
        unlisten?.();
        unlisten = null;
    });
};
