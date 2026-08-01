import { ref } from "vue";
import { RemotePlaylistClient } from "../core-client/remotePlaylistClient";
import {
    WebSocketCoreClient,
    type WebSocketCoreClientConnectionState,
} from "../core-client/webSocketCoreClient";

export const remoteConnectionState =
    ref<WebSocketCoreClientConnectionState>("idle");

export const remoteCoreClient = new WebSocketCoreClient({
    onConnectionStateChange: (state) => {
        remoteConnectionState.value = state;
    },
});

export const remotePlaylistClient = new RemotePlaylistClient(remoteCoreClient);
