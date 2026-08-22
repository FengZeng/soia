import { computed, onBeforeUnmount, onMounted, readonly, ref } from "vue";
import type { CastingClient } from "../core-client/CastingClient";
import type { CastDeviceDto } from "../core-client/generated/CastDeviceDto";
import type { CastSnapshotDto } from "../core-client/generated/CastSnapshotDto";

const emptySnapshot = (): CastSnapshotDto => ({
    revision: 0,
    phase: "idle",
    sessionId: null,
    device: null,
    mediaTitle: null,
    position: 0,
    duration: 0,
    volume: 100,
    muted: false,
    seekable: false,
    lastError: null,
});

export const useCasting = (client: CastingClient) => {
    const snapshot = ref<CastSnapshotDto>(emptySnapshot());
    const devices = ref<CastDeviceDto[]>([]);
    const isReady = ref(false);
    const isDiscovering = ref(false);
    const isConnecting = ref(false);
    const error = ref("");
    let unsubscribe: (() => void) | null = null;

    const isActive = computed(() => snapshot.value.sessionId !== null);
    const activeDeviceName = computed(() => snapshot.value.device?.name ?? "");

    const refresh = async () => {
        try {
            const [nextSnapshot, nextDevices] = await Promise.all([
                client.getSnapshot(),
                client.getDevices(),
            ]);
            snapshot.value = nextSnapshot;
            devices.value = nextDevices;
            error.value = "";
            isReady.value = true;
        } catch (cause) {
            error.value = toMessage(cause);
        }
    };

    const discover = async () => {
        isDiscovering.value = true;
        error.value = "";
        try {
            devices.value = await client.discover();
        } catch (cause) {
            error.value = toMessage(cause);
        } finally {
            isDiscovering.value = false;
        }
    };

    const connect = async (deviceId: string) => {
        isConnecting.value = true;
        error.value = "";
        try {
            snapshot.value = await client.connect(deviceId);
        } catch (cause) {
            error.value = toMessage(cause);
        } finally {
            isConnecting.value = false;
        }
    };

    const disconnect = async () => {
        try {
            snapshot.value = await client.disconnect();
            error.value = "";
        } catch (cause) {
            error.value = toMessage(cause);
        }
    };

    onMounted(() => {
        unsubscribe = client.subscribe(
            (nextSnapshot) => {
                if (nextSnapshot.revision >= snapshot.value.revision) {
                    snapshot.value = nextSnapshot;
                }
            },
            (nextDevices) => {
                devices.value = nextDevices;
            },
            (cause) => {
                error.value = cause.message;
            },
        );
        void refresh();
    });

    onBeforeUnmount(() => {
        unsubscribe?.();
        unsubscribe = null;
    });

    return {
        snapshot: readonly(snapshot),
        devices: readonly(devices),
        error: readonly(error),
        isReady: readonly(isReady),
        isDiscovering: readonly(isDiscovering),
        isConnecting: readonly(isConnecting),
        isActive,
        activeDeviceName,
        discover,
        connect,
        disconnect,
    };
};

const toMessage = (cause: unknown) => {
    if (cause && typeof cause === "object" && "message" in cause) {
        const message = (cause as { message?: unknown }).message;
        if (typeof message === "string" && message.trim()) return message;
    }
    return "casting request failed";
};
