import { invoke } from "@tauri-apps/api/core";
import type { CoreClient } from "../core-client/CoreClient";
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
  setFullscreen: (value: boolean) => Promise<void>;
};

export type ParsedPlaylistEntry = {
  path: string;
  title?: string | null;
  icon?: string | null;
};

export type ParsedPlaylistMetadata = {
  hasEndList: boolean;
  playlistType?: string | null;
  targetDuration?: number | null;
  hasHlsTags: boolean;
};

export type ParsedPlaylistFile = {
  entries: ParsedPlaylistEntry[];
  metadata: ParsedPlaylistMetadata;
};

export type LoadFileResult = {
  playbackKey?: string | null;
  title?: string | null;
  isLivePlayback?: boolean;
  superseded?: boolean;
};

export type ResolvedYoutubePlaylistEntry = {
  url: string;
  title?: string | null;
};

export type ResolvedYoutubePlaylist = {
  playlistTitle?: string | null;
  entries: ResolvedYoutubePlaylistEntry[];
};

export const usePlaybackCommands = (
  state: PlayerEffectState,
  currentWindow: CurrentWindow,
  coreClient: CoreClient,
) => {
  let lastAudibleVolume = 100;
  let volumeApplyQueue: Promise<void> = Promise.resolve();
  let volumeRequestId = 0;

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

  const normalizeParsedPlaylistFile = (
    response: Partial<ParsedPlaylistFile> | null | undefined,
  ): ParsedPlaylistFile => ({
    entries: Array.isArray(response?.entries) ? response.entries : [],
    metadata: {
      hasEndList: response?.metadata?.hasEndList === true,
      playlistType: response?.metadata?.playlistType ?? null,
      targetDuration:
        typeof response?.metadata?.targetDuration === "number"
          ? response.metadata.targetDuration
          : null,
      hasHlsTags: response?.metadata?.hasHlsTags === true,
    },
  });

  const parsePlaylistFile = async (path: string): Promise<ParsedPlaylistFile> => {
    const response = await invoke<ParsedPlaylistFile>(
      "parse_playlist_file",
      { payload: { path } },
    );
    return normalizeParsedPlaylistFile(response);
  };

  const parsePlaylistSource = async (
    source: string,
  ): Promise<ParsedPlaylistFile> => {
    const response = await invoke<ParsedPlaylistFile>(
      "parse_playlist_source",
      { payload: { source } },
    );
    return normalizeParsedPlaylistFile(response);
  };

  const resolveYoutubePlaylist = async (
    url: string,
  ): Promise<ResolvedYoutubePlaylist> => {
    return await invoke<ResolvedYoutubePlaylist>(
      "resolve_youtube_playlist",
      { payload: { url } },
    );
  };

  const togglePlayPause = async (): Promise<void> => {
    await coreClient.execute({
      type: "setPaused",
      paused: state.playback.isPlaying,
    });
  };

  const toggleFullscreen = async (): Promise<void> => {
    const isFull = await currentWindow.isFullscreen();
    await currentWindow.setFullscreen(!isFull);
    state.window.isFullscreen = !isFull;
  };

  type MpvArg = string | number | boolean;
  const runMpvCommand = async (args: MpvArg[]): Promise<void> => {
    await invoke("mpv_run_command", { args });
  };

  const stopPlayback = async (): Promise<void> => {
    await coreClient.execute({ type: "stop" });
  };

  const syncFullscreen = async (): Promise<void> => {
    state.window.isFullscreen = await currentWindow.isFullscreen();
  };

  const syncMpvRenderTarget = async (): Promise<void> => {
    await invoke("sync_mpv_render_target");
  };

  const seek = async (position: number): Promise<void> => {
    await coreClient.execute({ type: "seekAbsolute", position });
  };

  const seekRelative = async (position: number): Promise<void> => {
    await coreClient.execute({ type: "seekRelative", seconds: position });
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
        await coreClient.execute({ type: "setVolume", volume: nextVolume });
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
    parsePlaylistFile,
    parsePlaylistSource,
    resolveYoutubePlaylist,
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
