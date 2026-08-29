import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    CastingClient,
    CastingClientError,
    CastingDevicesListener,
    CastingSnapshotListener,
} from "./CastingClient";
import type { CastDeviceDto } from "./generated/CastDeviceDto";
import type { CastSnapshotDto } from "./generated/CastSnapshotDto";

const CAST_SNAPSHOT_EVENT = "cast-snapshot";
const CAST_DEVICES_EVENT = "cast-devices";

const toError = (error: unknown, fallback: string): CastingClientError => {
    const objectMessage =
        typeof error === "object" && error !== null && "message" in error
            ? (error as { message?: unknown }).message
            : undefined;
    return {
        message:
            error instanceof Error && error.message.trim()
                ? error.message
                : typeof error === "string" && error.trim()
                  ? error
                  : typeof objectMessage === "string" && objectMessage.trim()
                    ? objectMessage
                    : fallback,
    };
};

export class TauriCastingClient implements CastingClient {
    async getSnapshot(): Promise<CastSnapshotDto> {
        try {
            return await invoke<CastSnapshotDto>("get_cast_snapshot");
        } catch (error) {
            throw toError(error, "failed to retrieve casting status");
        }
    }

    async getDevices(): Promise<CastDeviceDto[]> {
        try {
            return await invoke<CastDeviceDto[]>("get_cast_devices");
        } catch (error) {
            throw toError(error, "failed to retrieve cast devices");
        }
    }

    async discover(): Promise<CastDeviceDto[]> {
        try {
            return await invoke<CastDeviceDto[]>("discover_cast_devices");
        } catch (error) {
            throw toError(error, "failed to discover cast devices");
        }
    }

    async connect(deviceId: string): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("connect_cast_device", { deviceId }, "failed to connect cast device");
    }

    async pause(): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("cast_pause", {}, "failed to pause casting");
    }

    async play(): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("cast_play", {}, "failed to play casting");
    }

    async seek(position: number): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("cast_seek", { position }, "failed to seek casting");
    }

    async stop(): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("cast_stop", {}, "failed to stop casting");
    }

    async setVolume(volume: number): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("set_cast_volume", { volume }, "failed to set cast volume");
    }

    async disconnect(): Promise<CastSnapshotDto> {
        return this.invokeSnapshot("disconnect_casting", {}, "failed to disconnect casting");
    }

    subscribe(
        onSnapshot: CastingSnapshotListener,
        onDevices: CastingDevicesListener,
        onError?: (error: CastingClientError) => void,
    ): () => void {
        let disposed = false;
        let unlistenSnapshot: UnlistenFn | null = null;
        let unlistenDevices: UnlistenFn | null = null;
        void listen<CastSnapshotDto>(CAST_SNAPSHOT_EVENT, (event) => {
            if (!disposed) onSnapshot(event.payload);
        })
            .then((unlisten) => {
                if (disposed) unlisten();
                else unlistenSnapshot = unlisten;
            })
            .catch((error) => onError?.(toError(error, "failed to subscribe to casting status")));
        void listen<CastDeviceDto[]>(CAST_DEVICES_EVENT, (event) => {
            if (!disposed) onDevices(event.payload ?? []);
        })
            .then((unlisten) => {
                if (disposed) unlisten();
                else unlistenDevices = unlisten;
            })
            .catch((error) => onError?.(toError(error, "failed to subscribe to cast devices")));
        return () => {
            disposed = true;
            unlistenSnapshot?.();
            unlistenDevices?.();
        };
    }

    private async invokeSnapshot(
        command: string,
        payload: Record<string, unknown>,
        fallback: string,
    ): Promise<CastSnapshotDto> {
        try {
            return await invoke<CastSnapshotDto>(command, payload);
        } catch (error) {
            throw toError(error, fallback);
        }
    }
}

export const tauriCastingClient = new TauriCastingClient();
