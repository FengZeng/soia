import { ref } from "vue";
import type { CoreClient } from "../core-client/CoreClient";
import {
    WebSocketCoreClient,
    type WebSocketCoreClientConnectionState,
} from "../core-client/webSocketCoreClient";

export const remoteConnectionState =
    ref<WebSocketCoreClientConnectionState>("idle");

export const remoteCoreClient: CoreClient = new WebSocketCoreClient({
    onConnectionStateChange: (state) => {
        remoteConnectionState.value = state;
    },
});
