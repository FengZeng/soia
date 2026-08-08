import { invoke } from "@tauri-apps/api/core";
import type { PlaybackController } from "../features/playback/playbackController";
import { open } from "@tauri-apps/plugin-dialog";
import { MEDIA_FILE_EXTENSIONS } from "../constants/media";

type PlayerEffectState = {
  media: {
    url: string;
  };
  playback: {
    isPlaying: boolean;
    volume: number;
  };
  window: {
    isFullscreen: boolean;
  };
};

type CurrentWindow = {
  isFullscreen: () => Promise<boolean>;
  maximize: () => Promise<void>;
  setFullscreen: (value: boolean) => Promise<void>;
};

export type LoadFileResult = {
  playbackKey?: string | null;
  title?: string | null;
  isLivePlayback?: boolean;
  superseded?: boolean;
};

export const usePlaybackCommands = (
  state: PlayerEffectState,
  currentWindow: CurrentWindow,
  playbackController: Pick<PlaybackController, "execute">,
  isWindowsPlatform = false,
) => {
  let lastAudibleVolume = 100;
  let volumeApplyQueue: Promise<void> = Promise.resolve();
  let volumeRequestId = 0;
  let restoreMaximizedAfterFullscreen = false;

  const MEDIA_FILES_FILTER = [
    {
      name: "Media Files",
      extensions: [...MEDIA_FILE_EXTENSIONS],
    },
  ];

  const normalizeSelectedPaths = (selected: string | string[] | null): string[] => {
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  };

  const openVideoPicker = async (): Promise<string[]> => {
    const selected = await open({
      multiple: true,
      directory: false,
      filters: MEDIA_FILES_FILTER,
    });
    return normalizeSelectedPaths(selected);
  };

  const loadPlaybackSource = async (
    keyOrUrl: string,
    preferredTitle?: string,
  ): Promise<LoadFileResult> => {
    return await invoke<LoadFileResult>("load_playback_source", {
      payload: { keyOrUrl, preferredTitle },
    });
  };

  const pickMediaPathsAuto = async (): Promise<string[]> => {
    const selected = await invoke<string[]>("pick_media_paths_native");
    return Array.isArray(selected) ? selected : [];
  };

  const pickFiles = async (): Promise<string[]> => {
    return openVideoPicker();
  };

  const togglePlayPause = async (): Promise<void> => {
    await playbackController.execute({
      type: "setPaused",
      paused: state.playback.isPlaying,
    });
  };

  const toggleFullscreen = async (): Promise<void> => {
    const isFull = await currentWindow.isFullscreen();
    if (isFull) {
      const shouldRestoreMaximized = restoreMaximizedAfterFullscreen;
      restoreMaximizedAfterFullscreen = false;
      try {
        await currentWindow.setFullscreen(false);
      } catch (error) {
        restoreMaximizedAfterFullscreen = shouldRestoreMaximized;
        throw error;
      }
      state.window.isFullscreen = false;
      if (shouldRestoreMaximized) {
        await currentWindow.maximize();
      }
      return;
    }

    const wasMaximized =
      isWindowsPlatform &&
      (await invoke<boolean>("prepare_window_for_fullscreen"));
    restoreMaximizedAfterFullscreen = wasMaximized;
    try {
      await currentWindow.setFullscreen(true);
    } catch (error) {
      restoreMaximizedAfterFullscreen = false;
      if (wasMaximized) {
        await currentWindow.maximize().catch(() => {});
      }
      throw error;
    }
    state.window.isFullscreen = true;
  };

  type MpvArg = string | number | boolean;
  const runMpvCommand = async (args: MpvArg[]): Promise<void> => {
    await invoke("mpv_run_command", { args });
  };

  const stopPlayback = async (): Promise<void> => {
    await playbackController.execute({ type: "stop" });
  };

  const syncFullscreen = async (): Promise<void> => {
    const isFullscreen = await currentWindow.isFullscreen();
    const exitedFullscreen = state.window.isFullscreen && !isFullscreen;
    state.window.isFullscreen = isFullscreen;
    if (
      isWindowsPlatform &&
      exitedFullscreen &&
      restoreMaximizedAfterFullscreen
    ) {
      restoreMaximizedAfterFullscreen = false;
      await currentWindow.maximize();
    }
  };

  const syncMpvRenderTarget = async (): Promise<void> => {
    await invoke("sync_mpv_render_target");
  };

  const seek = async (position: number): Promise<void> => {
    await playbackController.execute({ type: "seekAbsolute", position });
  };

  const seekRelative = async (position: number): Promise<void> => {
    await playbackController.execute({ type: "seekRelative", seconds: position });
  };

  const setLoopFile = async (enabled: boolean): Promise<void> => {
    await runMpvCommand(["set", "loop-file", enabled ? "inf" : "no"]);
  };

  const setVolume = async (volume: number): Promise<void> => {
    const nextVolume = Math.max(0, Math.min(100, Math.round(volume)));
    const requestId = ++volumeRequestId;
    state.playback.volume = nextVolume;
    if (nextVolume > 0) {
      lastAudibleVolume = nextVolume;
    }
    volumeApplyQueue = volumeApplyQueue
      .catch(() => {})
      .then(async () => {
        if (requestId !== volumeRequestId) return;
        await playbackController.execute({ type: "setVolume", volume: nextVolume });
      });
    await volumeApplyQueue;
  };

  const toggleMuted = async (): Promise<void> => {
    if (state.playback.volume > 0) {
      await setVolume(0);
      return;
    }
    await setVolume(lastAudibleVolume || 100);
  };

  return {
    loadPlaybackSource,
    pickMediaPathsAuto,
    pickFiles,
    togglePlayPause,
    toggleFullscreen,
    stopPlayback,
    syncFullscreen,
    syncMpvRenderTarget,
    seek,
    seekRelative,
    setLoopFile,
    setVolume,
    toggleMuted,
  };
};
