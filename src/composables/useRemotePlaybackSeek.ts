import { onMounted, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type RemotePlaybackSeekPayload = {
    relative: boolean;
    position: number;
};

type UseRemotePlaybackSeekOptions = {
    onSeek: (position: number) => Promise<void>;
    onSeekRelative: (position: number) => Promise<void>;
};

const REMOTE_PLAYBACK_SEEK_EVENT = "soia-remote-playback-seek";

export const useRemotePlaybackSeek = ({
    onSeek,
    onSeekRelative,
}: UseRemotePlaybackSeekOptions) => {
    let unlisten: UnlistenFn | null = null;

    onMounted(async () => {
        unlisten = await listen<RemotePlaybackSeekPayload>(
            REMOTE_PLAYBACK_SEEK_EVENT,
            ({ payload }) => {
                const action = payload.relative ? onSeekRelative : onSeek;
                void action(payload.position);
            },
        );
    });

    onUnmounted(() => {
        unlisten?.();
        unlisten = null;
    });
};
