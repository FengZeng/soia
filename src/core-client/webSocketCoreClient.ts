import type {
    CoreClient,
    CoreClientError,
    CoreClientErrorListener,
    CoreClientSnapshotListener,
    CoreClientUnsubscribe,
} from "./CoreClient";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";
import type { PlaybackSnapshotDto } from "./generated/PlaybackSnapshotDto";
import type { PlaylistSnapshotDto } from "./generated/PlaylistSnapshotDto";
import type { PlaylistEntriesPageDto } from "./generated/PlaylistEntriesPageDto";
import type { GetPlaylistEntriesPageDto } from "./generated/GetPlaylistEntriesPageDto";
import type { DeletePlaylistDto } from "./generated/DeletePlaylistDto";
import type { ImportPlaylistFromSourceDto } from "./generated/ImportPlaylistFromSourceDto";
import type { PlayPlaylistEntryDto } from "./generated/PlayPlaylistEntryDto";
import type { PlaylistMutationResultDto } from "./generated/PlaylistMutationResultDto";
import { PlaybackCommandContext } from "./playbackCommandContext";
import {
    isNewerSnapshot,
    parseWebSocketServerMessage,
    WEBSOCKET_PROTOCOL_VERSION,
} from "./webSocketProtocol";

export type WebSocketCoreClientConnectionState =
    | "idle"
    | "pairing"
    | "connecting"
    | "connected"
    | "reconnecting"
    | "incompatible"
    | "failed"
    | "closed";

export type WebSocketCoreClientOptions = {
    clientId?: string;
    webSocketUrl?: string;
    pairingUrl?: string;
    pairingCode?: string | null;
    reconnectDelayMs?: number;
    onConnectionStateChange?: (
        state: WebSocketCoreClientConnectionState,
    ) => void;
};

type PendingCommand = {
    envelope: ReturnType<PlaybackCommandContext["createEnvelope"]>;
    resolve: (result: CommandResultDto) => void;
    reject: (error: CoreClientError) => void;
    sent: boolean;
};

type SnapshotWaiter = {
    resolve: (snapshot: PlaybackSnapshotDto) => void;
    reject: (error: CoreClientError) => void;
};

type PendingPlaylistSnapshot = {
    resolve: (snapshot: PlaylistSnapshotDto) => void;
    reject: (error: CoreClientError) => void;
    sent: boolean;
};

type PendingPlaylistRequest<T> = {
    message: Record<string, unknown>;
    resolve: (result: T) => void;
    reject: (error: CoreClientError) => void;
    sent: boolean;
    retryOnReconnect: boolean;
};

const DEFAULT_RECONNECT_DELAY_MS = 1500;

const createClientId = () => {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
        return crypto.randomUUID();
    }
    return `remote-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
};

const getDefaultWebSocketUrl = () => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    return `${protocol}//${window.location.host}/ws`;
};

const getDefaultPairingUrl = () =>
    new URL("/api/pair", window.location.href).toString();

const getPairCodeFromLocation = () =>
    new URLSearchParams(window.location.hash.slice(1)).get("pair");

export class WebSocketCoreClient implements CoreClient {
    private readonly commandContext: PlaybackCommandContext;
    private readonly webSocketUrl: string;
    private readonly pairingUrl: string;
    private readonly reconnectDelayMs: number;
    private readonly onConnectionStateChange?: WebSocketCoreClientOptions["onConnectionStateChange"];
    private pairingCode: string | null;
    private socket: WebSocket | null = null;
    private connectPromise: Promise<void> | null = null;
    private reconnectTimer: number | null = null;
    private connectionState: WebSocketCoreClientConnectionState = "idle";
    private snapshot: PlaybackSnapshotDto | null = null;
    private protocolReady = false;
    private disposed = false;
    private readonly subscriptions = new Map<
        CoreClientSnapshotListener,
        CoreClientErrorListener | undefined
    >();
    private readonly pendingCommands = new Map<string, PendingCommand>();
    private readonly snapshotWaiters: SnapshotWaiter[] = [];
    private readonly pendingPlaylistSnapshots = new Map<string, PendingPlaylistSnapshot>();
    private readonly playlistSubscriptions = new Set<(snapshot: PlaylistSnapshotDto) => void>();
    private readonly pendingPlaylistEntries = new Map<string, PendingPlaylistRequest<PlaylistEntriesPageDto>>();
    private readonly pendingPlaylistDeletes = new Map<string, PendingPlaylistRequest<PlaylistMutationResultDto>>();
    private readonly pendingPlaylistImports = new Map<string, PendingPlaylistRequest<PlaylistMutationResultDto>>();
    private readonly pendingPlaylistPlays = new Map<string, PendingPlaylistRequest<CommandResultDto>>();
    private playlistSnapshot: PlaylistSnapshotDto | null = null;

    constructor(options: WebSocketCoreClientOptions = {}) {
        this.commandContext = new PlaybackCommandContext(
            options.clientId ?? createClientId(),
        );
        this.webSocketUrl = options.webSocketUrl ?? getDefaultWebSocketUrl();
        this.pairingUrl = options.pairingUrl ?? getDefaultPairingUrl();
        this.pairingCode =
            options.pairingCode === undefined
                ? getPairCodeFromLocation()
                : options.pairingCode;
        this.reconnectDelayMs =
            options.reconnectDelayMs ?? DEFAULT_RECONNECT_DELAY_MS;
        this.onConnectionStateChange = options.onConnectionStateChange;
    }

    getSnapshot(): Promise<PlaybackSnapshotDto> {
        if (this.disposed) {
            return Promise.reject(this.transportError("WebSocket client is disposed"));
        }
        if (this.snapshot) return Promise.resolve(this.snapshot);
        const snapshotPromise = new Promise<PlaybackSnapshotDto>((resolve, reject) => {
            this.snapshotWaiters.push({ resolve, reject });
        });
        void this.connect().catch(() => {
            // Pairing and terminal protocol failures reject snapshot waiters.
            // Transient socket failures continue through the reconnect loop.
        });
        return snapshotPromise;
    }

    subscribe(
        listener: CoreClientSnapshotListener,
        onError?: CoreClientErrorListener,
    ): CoreClientUnsubscribe {
        this.subscriptions.set(listener, onError);
        if (this.snapshot) {
            listener(this.snapshot);
        }
        void this.connect().catch((error) => {
            this.notifyError(error as CoreClientError);
        });
        return () => {
            this.subscriptions.delete(listener);
        };
    }

    execute(command: PlaybackCommandDto): Promise<CommandResultDto> {
        const envelope = this.commandContext.createEnvelope(command);
        return new Promise<CommandResultDto>((resolve, reject) => {
            this.pendingCommands.set(envelope.commandId, {
                envelope,
                resolve,
                reject,
                sent: false,
            });
            this.sendPendingCommands();
            void this.connect().catch(() => {
                // Transient failures retain the envelope for retry. Terminal
                // pairing or protocol failures reject pending commands directly.
            });
        });
    }

    getPlaylistSnapshot(): Promise<PlaylistSnapshotDto> {
        if (this.disposed) {
            return Promise.reject(this.transportError("WebSocket client is disposed"));
        }
        const requestId = `playlist-${createClientId()}`;
        return new Promise<PlaylistSnapshotDto>((resolve, reject) => {
            this.pendingPlaylistSnapshots.set(requestId, {
                resolve,
                reject,
                sent: false,
            });
            this.sendPendingPlaylistSnapshots();
            void this.connect().catch(() => {});
        });
    }

    subscribePlaylist(listener: (snapshot: PlaylistSnapshotDto) => void): () => void {
        this.playlistSubscriptions.add(listener);
        if (this.playlistSnapshot) {
            listener(this.playlistSnapshot);
        } else {
            void this.getPlaylistSnapshot().catch((error) => {
                this.notifyError(error as CoreClientError);
            });
        }
        return () => this.playlistSubscriptions.delete(listener);
    }

    getPlaylistEntriesPage(request: GetPlaylistEntriesPageDto): Promise<PlaylistEntriesPageDto> {
        return this.sendPlaylistRequest("playlistEntriesPage", { request }, this.pendingPlaylistEntries, true);
    }

    deletePlaylist(request: DeletePlaylistDto): Promise<PlaylistMutationResultDto> {
        return this.sendPlaylistRequest("deletePlaylist", { request }, this.pendingPlaylistDeletes, true);
    }

    importPlaylistFromSource(request: ImportPlaylistFromSourceDto): Promise<PlaylistMutationResultDto> {
        return this.sendPlaylistRequest("importPlaylistFromSource", { request }, this.pendingPlaylistImports, true);
    }

    playPlaylistEntry(request: PlayPlaylistEntryDto): Promise<CommandResultDto> {
        if (this.disposed) return Promise.reject(this.transportError("WebSocket client is disposed"));
        if (this.pendingPlaylistPlays.has(request.commandId)) {
            return Promise.reject(this.protocolError("duplicate playlist playback command id"));
        }
        return new Promise<CommandResultDto>((resolve, reject) => {
            this.pendingPlaylistPlays.set(request.commandId, {
                message: { type: "playPlaylistEntry", request },
                resolve,
                reject,
                sent: false,
                retryOnReconnect: true,
            });
            this.sendPendingPlaylistRequests();
            void this.connect().catch(() => {});
        });
    }

    connect(): Promise<void> {
        if (this.disposed) {
            return Promise.reject(this.transportError("WebSocket client is disposed"));
        }
        if (this.connectionState === "incompatible") {
            return Promise.reject(this.protocolError("remote protocol is incompatible"));
        }
        if (this.protocolReady) return Promise.resolve();
        if (this.connectPromise) return this.connectPromise;

        this.connectPromise = this.connectInternal().finally(() => {
            this.connectPromise = null;
        });
        return this.connectPromise;
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        if (this.reconnectTimer !== null) {
            window.clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
        }
        this.socket?.close();
        this.socket = null;
        this.protocolReady = false;
        this.setConnectionState("closed");
        this.rejectPending(this.transportError("WebSocket client is disposed"));
        this.rejectPendingPlaylistSnapshots(
            this.transportError("WebSocket client is disposed"),
        );
        this.rejectPendingPlaylistRequests(this.transportError("WebSocket client is disposed"));
        this.rejectSnapshotWaiters(this.transportError("WebSocket client is disposed"));
        this.subscriptions.clear();
    }

    private async connectInternal() {
        await this.completePairing();
        await this.openSocket();
    }

    private async completePairing() {
        if (!this.pairingCode) return;
        this.setConnectionState("pairing");
        try {
            const response = await fetch(this.pairingUrl, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ pairCode: this.pairingCode }),
            });
            if (!response.ok) {
                throw new Error("Pairing code expired");
            }
            this.pairingCode = null;
            window.history.replaceState(null, "", window.location.pathname);
        } catch (error) {
            const clientError = this.transportError(
                error instanceof Error ? error.message : "Could not pair with Soia.",
            );
            this.setConnectionState("failed");
            this.rejectPending(clientError);
            this.rejectPendingPlaylistSnapshots(clientError);
            this.rejectPendingPlaylistRequests(clientError);
            this.rejectSnapshotWaiters(clientError);
            throw clientError;
        }
    }

    private openSocket(): Promise<void> {
        this.setConnectionState(
            this.connectionState === "reconnecting" ? "reconnecting" : "connecting",
        );
        this.protocolReady = false;

        return new Promise<void>((resolve, reject) => {
            let opened = false;
            const socket = new WebSocket(this.webSocketUrl);
            this.socket = socket;

            socket.onopen = () => {
                opened = true;
            };
            socket.onmessage = (event) => {
                if (this.socket !== socket || this.disposed) return;
                this.handleMessage(socket, String(event.data), resolve, reject);
            };
            socket.onerror = () => {
                if (this.socket !== socket || this.disposed) return;
                this.notifyError(this.transportError("Could not connect to Soia."));
            };
            socket.onclose = () => {
                if (this.socket !== socket) return;
                this.socket = null;
                const wasProtocolReady = this.protocolReady;
                this.protocolReady = false;
                this.pendingCommands.forEach((command) => {
                    command.sent = false;
                });
                this.pendingPlaylistSnapshots.forEach((request) => {
                    request.sent = false;
                });
                this.resetPendingPlaylistRequests();
                if (!opened || !wasProtocolReady) {
                    reject(this.transportError("Could not connect to Soia."));
                }
                if (this.disposed || this.connectionState === "incompatible") return;
                this.setConnectionState("reconnecting");
                this.scheduleReconnect();
            };
        });
    }

    private handleMessage(
        socket: WebSocket,
        raw: string,
        resolveConnection: () => void,
        rejectConnection: (error: CoreClientError) => void,
    ) {
        let message;
        try {
            message = parseWebSocketServerMessage(raw);
        } catch (error) {
            const clientError = this.protocolError(
                error instanceof Error ? error.message : "received invalid WebSocket message",
            );
            this.setConnectionState("incompatible");
            this.rejectPending(clientError);
            this.rejectPendingPlaylistSnapshots(clientError);
            this.rejectPendingPlaylistRequests(clientError);
            this.rejectSnapshotWaiters(clientError);
            rejectConnection(clientError);
            socket.close();
            return;
        }

        if (message.type === "hello") {
            if (message.protocolVersion !== WEBSOCKET_PROTOCOL_VERSION) {
                const clientError = this.protocolError(
                    `incompatible remote protocol version: expected ${WEBSOCKET_PROTOCOL_VERSION}, received ${message.protocolVersion}`,
                );
                this.setConnectionState("incompatible");
                this.rejectPending(clientError);
                this.rejectPendingPlaylistSnapshots(clientError);
                this.rejectPendingPlaylistRequests(clientError);
                this.rejectSnapshotWaiters(clientError);
                rejectConnection(clientError);
                socket.close();
                return;
            }
            this.protocolReady = true;
            this.setConnectionState("connected");
            resolveConnection();
            this.sendPendingCommands();
            this.sendPendingPlaylistSnapshots();
            this.sendPendingPlaylistRequests();
            return;
        }

        if (!this.protocolReady) {
            const clientError = this.protocolError("received a message before protocol hello");
            this.setConnectionState("incompatible");
            this.rejectPending(clientError);
            this.rejectPendingPlaylistSnapshots(clientError);
            this.rejectPendingPlaylistRequests(clientError);
            this.rejectSnapshotWaiters(clientError);
            rejectConnection(clientError);
            socket.close();
            return;
        }

        switch (message.type) {
            case "state":
                this.handleSnapshot(message.state);
                break;
            case "playlistSnapshot": {
                this.handlePlaylistSnapshot(message.snapshot);
                if (!message.id) break;
                const pending = this.pendingPlaylistSnapshots.get(message.id);
                if (!pending) break;
                this.pendingPlaylistSnapshots.delete(message.id);
                pending.resolve(message.snapshot);
                break;
            }
            case "playlistSummaries":
                break;
            case "playlistEntriesPage":
                this.resolvePlaylistRequest(this.pendingPlaylistEntries, message.id, message.page);
                break;
            case "playlistDeleted":
                this.resolvePlaylistRequest(this.pendingPlaylistDeletes, message.id, {
                    playlist: null,
                    collectionRevision: message.collectionRevision,
                });
                break;
            case "playlistImported":
                this.resolvePlaylistRequest(this.pendingPlaylistImports, message.id, {
                    playlist: { summary: message.playlist, entries: [] },
                    collectionRevision: message.collectionRevision,
                });
                break;
            case "commandResult": {
                const pending = this.pendingCommands.get(message.result.commandId);
                if (pending) {
                    this.pendingCommands.delete(message.result.commandId);
                    pending.resolve(message.result);
                    break;
                }
                this.resolvePlaylistRequest(this.pendingPlaylistPlays, message.result.commandId, message.result);
                break;
            }
            case "error": {
                if (!message.id) {
                    this.notifyError({ type: "core", error: message.error });
                    break;
                }
                const pending = this.pendingCommands.get(message.id);
                if (pending) {
                    this.pendingCommands.delete(message.id);
                    pending.reject({ type: "core", error: message.error });
                    break;
                }
                if (this.rejectPlaylistRequest(message.id, { type: "core", error: message.error })) break;
                const snapshotRequest = this.pendingPlaylistSnapshots.get(message.id);
                if (snapshotRequest) {
                    this.pendingPlaylistSnapshots.delete(message.id);
                    snapshotRequest.reject({ type: "core", error: message.error });
                }
                break;
            }
            case "pong":
            case "navigationResult":
                break;
        }
    }

    private handleSnapshot(snapshot: PlaybackSnapshotDto) {
        if (!isNewerSnapshot(snapshot, this.snapshot)) return;
        this.snapshot = snapshot;
        this.commandContext.updateSnapshot(snapshot);
        const waiters = this.snapshotWaiters.splice(0);
        waiters.forEach((waiter) => waiter.resolve(snapshot));
        this.subscriptions.forEach((_onError, listener) => listener(snapshot));
    }

    private handlePlaylistSnapshot(snapshot: PlaylistSnapshotDto) {
        if (this.playlistSnapshot && snapshot.collectionRevision < this.playlistSnapshot.collectionRevision) return;
        this.playlistSnapshot = snapshot;
        this.playlistSubscriptions.forEach((listener) => listener(snapshot));
    }

    private sendPendingCommands() {
        if (!this.protocolReady || this.socket?.readyState !== WebSocket.OPEN) return;
        this.pendingCommands.forEach((command) => {
            if (command.sent) return;
            this.socket?.send(
                JSON.stringify({ type: "command", envelope: command.envelope }),
            );
            command.sent = true;
        });
    }

    private sendPendingPlaylistSnapshots() {
        if (!this.protocolReady || this.socket?.readyState !== WebSocket.OPEN) return;
        this.pendingPlaylistSnapshots.forEach((request, id) => {
            if (request.sent) return;
            this.socket?.send(JSON.stringify({ type: "playlistSnapshot", id }));
            request.sent = true;
        });
    }

    private sendPlaylistRequest<T>(
        type: string,
        body: Record<string, unknown>,
        pending: Map<string, PendingPlaylistRequest<T>>,
        retryOnReconnect: boolean,
    ): Promise<T> {
        if (this.disposed) return Promise.reject(this.transportError("WebSocket client is disposed"));
        const id = `playlist-${createClientId()}`;
        return new Promise<T>((resolve, reject) => {
            pending.set(id, {
                message: { type, id, ...body },
                resolve,
                reject,
                sent: false,
                retryOnReconnect,
            });
            this.sendPendingPlaylistRequests();
            void this.connect().catch(() => {});
        });
    }

    private sendPendingPlaylistRequests() {
        if (!this.protocolReady || this.socket?.readyState !== WebSocket.OPEN) return;
        [this.pendingPlaylistEntries, this.pendingPlaylistDeletes, this.pendingPlaylistImports, this.pendingPlaylistPlays]
            .forEach((pending) => pending.forEach((request) => {
                if (request.sent) return;
                this.socket?.send(JSON.stringify(request.message));
                request.sent = true;
            }));
    }

    private resolvePlaylistRequest<T>(
        pendingRequests: Map<string, PendingPlaylistRequest<T>>,
        id: string | null | undefined,
        result: T,
    ) {
        if (!id) return;
        const pending = pendingRequests.get(id);
        if (!pending) return;
        pendingRequests.delete(id);
        pending.resolve(result);
    }

    private rejectPlaylistRequest(id: string, error: CoreClientError) {
        const pendingRequests = [this.pendingPlaylistEntries, this.pendingPlaylistDeletes, this.pendingPlaylistImports, this.pendingPlaylistPlays];
        for (const pending of pendingRequests) {
            const request = pending.get(id);
            if (!request) continue;
            pending.delete(id);
            request.reject(error);
            return true;
        }
        return false;
    }

    private resetPendingPlaylistRequests() {
        [this.pendingPlaylistEntries, this.pendingPlaylistDeletes, this.pendingPlaylistImports, this.pendingPlaylistPlays]
            .forEach((pending) => pending.forEach((request, id) => {
                if (request.retryOnReconnect) {
                    request.sent = false;
                    return;
                }
                pending.delete(id);
                request.reject(this.transportError("playlist mutation connection was interrupted"));
            }));
    }

    private rejectPendingPlaylistRequests(error: CoreClientError) {
        [this.pendingPlaylistEntries, this.pendingPlaylistDeletes, this.pendingPlaylistImports, this.pendingPlaylistPlays]
            .forEach((pending) => {
                pending.forEach((request) => request.reject(error));
                pending.clear();
            });
    }

    private scheduleReconnect() {
        if (this.reconnectTimer !== null || this.disposed) return;
        this.reconnectTimer = window.setTimeout(() => {
            this.reconnectTimer = null;
            void this.connect().catch((error) => {
                this.notifyError(error as CoreClientError);
            });
        }, this.reconnectDelayMs);
    }

    private setConnectionState(state: WebSocketCoreClientConnectionState) {
        if (this.connectionState === state) return;
        this.connectionState = state;
        this.onConnectionStateChange?.(state);
    }

    private notifyError(error: CoreClientError) {
        this.subscriptions.forEach((onError) => onError?.(error));
    }

    private rejectPending(error: CoreClientError) {
        const pending = Array.from(this.pendingCommands.values());
        this.pendingCommands.clear();
        pending.forEach((command) => command.reject(error));
    }

    private rejectPendingPlaylistSnapshots(error: CoreClientError) {
        const pending = Array.from(this.pendingPlaylistSnapshots.values());
        this.pendingPlaylistSnapshots.clear();
        pending.forEach((request) => request.reject(error));
    }

    private rejectSnapshotWaiters(error: CoreClientError) {
        const waiters = this.snapshotWaiters.splice(0);
        waiters.forEach((waiter) => waiter.reject(error));
    }

    private transportError(message: string): CoreClientError {
        return { type: "transport", message };
    }

    private protocolError(message: string): CoreClientError {
        return { type: "protocol", message };
    }
}
