import { onMounted, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
    Window,
    LogicalSize,
    PhysicalPosition,
    currentMonitor,
    primaryMonitor,
} from "@tauri-apps/api/window";
import type { ProgressPayload } from "../types/media";
import type {
    CoreClient,
    CoreClientUnsubscribe,
} from "../core-client/CoreClient";
import type { PlaybackSnapshotDto } from "../core-client/generated/PlaybackSnapshotDto";
import type { PlayerApi } from "./usePlaybackController";

type EndFilePayload = {
    reason?: string;
};

type SourceLoadState = {
    loading: boolean;
    loadingKey: string | null;
    error: string | null;
};

type PlaybackLoadPreparedPayload = {
    playbackKey: string;
    resumePosition: number;
};

type PlayerEventApi = Pick<PlayerApi, "state" | "syncFullscreen">;

type TracksApi = {
    handleTracksSnapshot: (tracks: PlaybackSnapshotDto["tracks"]) => void;
};

type UiApi = {
    showControls: { value: boolean };
    onUserInteraction: () => void;
    onMouseMove: () => void;
    toggleControlsFromMiddleClick: () => void;
    resetInactivityTimer: () => void;
    cleanup: () => void;
};

type AppEventBindingsOptions = {
    coreClient: CoreClient;
    player: PlayerEventApi;
    tracks: TracksApi;
    ui: UiApi;
    onFullscreenTransition: () => void;
    onFullscreenTransitionEnd: () => void;
    onCloseAllMenus: (event: MouseEvent) => void;
    onKeydown: (event: KeyboardEvent) => void;
    onDoubleClick: (event: MouseEvent) => void;
    setWindowControlsVisible: (visible: boolean) => Promise<void>;
    onFileLoaded?: () => void | Promise<void>;
    onPlaybackRestart?: () => void | Promise<void>;
    onRemoteSeekStarted?: () => void;
    onRemoteSeekFailed?: () => void;
    onProgress?: (payload: ProgressPayload) => void;
    onEndFile?: (payload: EndFilePayload) => void | Promise<void>;
    onSourceLoadState?: (state: SourceLoadState) => void;
    onPlaybackLoadPrepared?: (payload: PlaybackLoadPreparedPayload) => void;
    onPlaybackSpeedChange?: (speed: number) => void;
    resolveMediaTitle?: (incomingTitle: string, currentUrl: string) => string;
};

const AUTO_RESIZE_MIN_WIDTH = 720;
const AUTO_RESIZE_MIN_HEIGHT = 480;

export const useAppEventBindings = ({
    coreClient,
    player,
    tracks,
    ui,
    onFullscreenTransition,
    onFullscreenTransitionEnd,
    onCloseAllMenus,
    onKeydown,
    onDoubleClick,
    setWindowControlsVisible,
    onFileLoaded,
    onPlaybackRestart,
    onRemoteSeekStarted,
    onRemoteSeekFailed,
    onProgress,
    onEndFile,
    onSourceLoadState,
    onPlaybackLoadPrepared,
    onPlaybackSpeedChange,
    resolveMediaTitle,
}: AppEventBindingsOptions) => {
    // 事件监听器引用
    let unlistenPlaybackSnapshot: CoreClientUnsubscribe | null = null;
    let unlistenPlaybackLoadPrepared: UnlistenFn | null = null;
    let unlistenFileLoaded: UnlistenFn | null = null;
    let unlistenPlaybackRestart: UnlistenFn | null = null;
    let unlistenRemoteSeekStarted: UnlistenFn | null = null;
    let unlistenRemoteSeekFailed: UnlistenFn | null = null;
    let unlistenResize: UnlistenFn | null = null;
    let unlistenWindowResized: UnlistenFn | null = null;
    let unlistenFullscreenWill: UnlistenFn | null = null;
    let unlistenEndFile: UnlistenFn | null = null;
    let unlistenMediaTitle: UnlistenFn | null = null;
    let unlistenHwdecCurrent: UnlistenFn | null = null;

    const windowEventHandlers: Array<[keyof WindowEventMap, EventListener]> = [
        ["mousemove", () => ui.onMouseMove()],
        [
            "mousedown",
            (event) => {
                const mouseEvent = event as MouseEvent;
                if (
                    mouseEvent.button === 1 &&
                    player.state.media.isFileLoaded
                ) {
                    mouseEvent.preventDefault();
                    ui.toggleControlsFromMiddleClick();
                    return;
                }
                ui.onUserInteraction();
            },
        ],
        [
            "auxclick",
            (event) => {
                const mouseEvent = event as MouseEvent;
                if (
                    mouseEvent.button === 1 &&
                    player.state.media.isFileLoaded
                ) {
                    mouseEvent.preventDefault();
                }
            },
        ],
        ["click", (event) => onCloseAllMenus(event as MouseEvent)],
        ["keydown", (event) => onKeydown(event as KeyboardEvent)],
        [
            "dblclick",
            (event) => {
                const mouseEvent = event as MouseEvent;
                if (mouseEvent.button === 0) onDoubleClick(mouseEvent);
            },
        ],
    ];

    let latestPlaybackSnapshotRevision = -1;
    const applyPlaybackSnapshot = (snapshot: PlaybackSnapshotDto) => {
        if (snapshot.revision < latestPlaybackSnapshotRevision) return;
        latestPlaybackSnapshotRevision = snapshot.revision;
        player.state.playback.currentTime = snapshot.position;
        player.state.playback.duration = snapshot.duration;
        player.state.playback.bufferedTime =
            typeof snapshot.bufferedPosition === "number" &&
            Number.isFinite(snapshot.bufferedPosition)
                ? snapshot.bufferedPosition
                : snapshot.position;
        player.state.playback.isPlaying = snapshot.isPlaying;
        player.state.playback.isBuffering = snapshot.isBuffering === true;
        player.state.playback.downloadSpeedBps =
            typeof snapshot.downloadSpeedBps === "number" &&
            Number.isFinite(snapshot.downloadSpeedBps)
                ? Math.max(0, snapshot.downloadSpeedBps)
                : 0;
        player.state.playback.volume = snapshot.volume;
        tracks.handleTracksSnapshot(snapshot.tracks);
        onPlaybackSpeedChange?.(snapshot.speed);
        onSourceLoadState?.({
            loading: snapshot.sourceLoading,
            loadingKey: snapshot.sourceLoadingKey,
            error: snapshot.sourceLoadError,
        });
        if (onProgress) {
            onProgress({
                time_pos: snapshot.position,
                duration: snapshot.duration,
                buffered_pos: snapshot.bufferedPosition,
                is_playing: snapshot.isPlaying,
                video_bitrate: player.state.playback.videoBitrate,
                is_buffering: snapshot.isBuffering,
                download_speed_bps: player.state.playback.downloadSpeedBps,
            });
        }
    };

    onMounted(async () => {
        const currentWindow = Window.getCurrent();
        unlistenWindowResized = await currentWindow.onResized(async () => {
            await player.syncFullscreen();
            onFullscreenTransitionEnd();
        });

        unlistenFullscreenWill = await listen("fullscreen-will-change", () => {
            onFullscreenTransition();
        });

        unlistenPlaybackSnapshot = coreClient.subscribe(applyPlaybackSnapshot);
        void coreClient.getSnapshot().then(applyPlaybackSnapshot).catch(() => {});

        unlistenPlaybackLoadPrepared = await listen<PlaybackLoadPreparedPayload>(
            "playback-load-prepared",
            (event) => onPlaybackLoadPrepared?.(event.payload),
        );

        // 监听文件加载完成
        unlistenFileLoaded = await listen("file_loaded", () => {
            player.state.media.isFileLoaded = true;
            player.state.media.lastLoadedUrl = player.state.media.url;
            ui.resetInactivityTimer();
            if (onFileLoaded) {
                void onFileLoaded();
            }
        });

        unlistenPlaybackRestart = await listen("mpv-playback-restart", () => {
            if (onPlaybackRestart) {
                void onPlaybackRestart();
            }
        });

        unlistenRemoteSeekStarted = await listen("remote-seek-started", () => {
            onRemoteSeekStarted?.();
        });

        unlistenRemoteSeekFailed = await listen("remote-seek-failed", () => {
            onRemoteSeekFailed?.();
        });

        unlistenEndFile = await listen<EndFilePayload>("mpv-end-file", (event) => {
            if (onEndFile) {
                void onEndFile(event.payload ?? {});
            }
        });

        unlistenMediaTitle = await listen<string>("mpv-media-title", (event) => {
            const incomingTitle =
                typeof event.payload === "string" ? event.payload.trim() : "";
            const title = resolveMediaTitle
                ? resolveMediaTitle(incomingTitle, player.state.media.url)
                : incomingTitle;
            player.state.media.title = title;
        });

        unlistenHwdecCurrent = await listen<string>(
            "mpv-hwdec-current",
            (event) => {
                const hwdec =
                    typeof event.payload === "string"
                        ? event.payload.trim()
                        : "";
                player.state.playback.hwdecCurrent = hwdec;
            },
        );

        // Listen for resize events
        unlistenResize = await listen<[number, number]>(
            "resize_window",
            async ({ payload }) => {
                const [width, height] = payload;
                if (width <= 0 || height <= 0) return;
                if (player.state.window.isFullscreen) return;
                try {
                    const isWindowFullscreen = await currentWindow.isFullscreen();
                    if (isWindowFullscreen) return;
                } catch {
                    // Ignore fullscreen detection errors and continue fallback logic.
                }

                const monitor =
                    (await currentMonitor()) ?? (await primaryMonitor());
                const scale = await currentWindow.scaleFactor();
                const [innerSize, outerSize] = await Promise.all([
                    currentWindow.innerSize(),
                    currentWindow.outerSize(),
                ]);
                const frameW = Math.max(
                    0,
                    (outerSize.width - innerSize.width) / scale,
                );
                const frameH = Math.max(
                    0,
                    (outerSize.height - innerSize.height) / scale,
                );
                const workAreaSize = monitor?.workArea?.size ?? monitor?.size;
                const workAreaPos =
                    monitor?.workArea?.position ?? monitor?.position;
                const workAreaW = workAreaSize
                    ? workAreaSize.width / scale
                    : null;
                const workAreaH = workAreaSize
                    ? workAreaSize.height / scale
                    : null;
                const maxW =
                    workAreaW !== null
                        ? Math.max(1, Math.floor(workAreaW - frameW))
                        : Math.max(1, Math.floor(width / scale));
                const maxH =
                    workAreaH !== null
                        ? Math.max(1, Math.floor(workAreaH - frameH))
                        : Math.max(1, Math.floor(height / scale));
                const minW = Math.min(AUTO_RESIZE_MIN_WIDTH, maxW);
                const minH = Math.min(AUTO_RESIZE_MIN_HEIGHT, maxH);

                // MPV width/height are video pixel dimensions. Convert to logical size
                // so 1920x1080 maps to 960x540 on a 2x Retina display.
                const logicalWidth = width / scale;
                const logicalHeight = height / scale;
                const ratio = logicalWidth / logicalHeight;
                let targetW = Math.min(logicalWidth, maxW);
                let targetH = targetW / ratio;
                if (targetH > maxH) {
                    targetH = maxH;
                    targetW = targetH * ratio;
                }
                if (targetW < minW || targetH < minH) {
                    const scaleUp = Math.max(minW / targetW, minH / targetH);
                    targetW *= scaleUp;
                    targetH *= scaleUp;
                    if (targetW > maxW || targetH > maxH) {
                        const scaleDown = Math.min(maxW / targetW, maxH / targetH);
                        targetW *= scaleDown;
                        targetH *= scaleDown;
                    }
                }
                targetW = Math.min(maxW, Math.max(minW, targetW));
                targetH = Math.min(maxH, Math.max(minH, targetH));

                await currentWindow.setSize(
                    new LogicalSize(Math.floor(targetW), Math.floor(targetH)),
                );
                await currentWindow.center();

                if (
                    workAreaSize !== undefined &&
                    workAreaPos !== undefined &&
                    workAreaSize !== null &&
                    workAreaPos !== null
                ) {
                    const [outerPos, outerSizeAfterResize] = await Promise.all([
                        currentWindow.outerPosition(),
                        currentWindow.outerSize(),
                    ]);

                    const minX = workAreaPos.x;
                    const minY = workAreaPos.y;
                    const maxX =
                        workAreaPos.x +
                        Math.max(0, workAreaSize.width - outerSizeAfterResize.width);
                    const maxY =
                        workAreaPos.y +
                        Math.max(0, workAreaSize.height - outerSizeAfterResize.height);
                    const clampedX = Math.min(Math.max(outerPos.x, minX), maxX);
                    const clampedY = Math.min(Math.max(outerPos.y, minY), maxY);

                    if (clampedX !== outerPos.x || clampedY !== outerPos.y) {
                        await currentWindow.setPosition(
                            new PhysicalPosition(clampedX, clampedY),
                        );
                    }
                }
            },
        );

        // 全局交互监听
        windowEventHandlers.forEach(([eventName, handler]) => {
            window.addEventListener(eventName, handler);
        });
        await setWindowControlsVisible(ui.showControls.value);
    });

    onUnmounted(() => {
        unlistenPlaybackSnapshot?.();
        unlistenPlaybackLoadPrepared?.();
        unlistenFileLoaded?.();
        unlistenPlaybackRestart?.();
        unlistenRemoteSeekStarted?.();
        unlistenRemoteSeekFailed?.();
        unlistenResize?.();
        unlistenWindowResized?.();
        unlistenFullscreenWill?.();
        unlistenEndFile?.();
        unlistenMediaTitle?.();
        unlistenHwdecCurrent?.();
        ui.cleanup();
        windowEventHandlers.forEach(([eventName, handler]) => {
            window.removeEventListener(eventName, handler);
        });
    });
};
