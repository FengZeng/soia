import { computed, reactive } from "vue";
import { Window } from "@tauri-apps/api/window";
import type { CoreClient } from "../core-client/CoreClient";
import {
  usePlaybackCommands,
  type LoadFileResult,
} from "./usePlaybackCommands";
import { formatTime } from "../utils/formatTime";

type PlayerState = {
  media: {
    url: string;
    lastLoadedUrl: string;
    isFileLoaded: boolean;
    isLivePlayback: boolean;
    title: string;
  };
  playback: {
    isPlaying: boolean;
    isBuffering: boolean;
    downloadSpeedBps: number;
    currentTime: number;
    duration: number;
    bufferedTime: number;
    videoBitrate: number;
    hwdecCurrent: string;
    volume: number;
  };
  window: {
    isFullscreen: boolean;
  };
};

export type PlayerApi = {
  state: PlayerState;
  progressPercent: { value: number };
  bufferedPercent: { value: number };
  isUrlModified: { value: boolean };
  formatTime: (seconds: number) => string;
  loadPlaybackSource: (
    keyOrUrl: string,
    preferredTitle?: string,
  ) => Promise<LoadFileResult>;
  pickMediaPathsAuto: () => Promise<string[]>;
  pickFiles: () => Promise<string[]>;
  togglePlayPause: () => Promise<void>;
  toggleFullscreen: () => Promise<void>;
  stopPlayback: () => Promise<void>;
  syncFullscreen: () => Promise<void>;
  syncMpvRenderTarget: () => Promise<void>;
  seek: (position: number) => Promise<void>;
  seekRelative: (position: number) => Promise<void>;
  setLoopFile: (enabled: boolean) => Promise<void>;
  setVolume: (volume: number) => Promise<void>;
  toggleMuted: () => Promise<void>;
};

export const usePlaybackController = (coreClient: CoreClient): PlayerApi => {
  const currentWindow = Window.getCurrent();
  const isWindowsPlatform =
    typeof navigator !== "undefined" && /\bwindows\b/i.test(navigator.userAgent);

  const state = reactive<PlayerState>({
    media: {
      // url: "/Users/feng/video/test1080x1080.mp4",
      // url: "/Users/feng/video/DolbyVision/Shogun.S01E01.2024.2160p.DSNP.WEB-DL.H265.DV.DDP5.1.mkv",
      url: "",
      lastLoadedUrl: "",
      isFileLoaded: false,
      isLivePlayback: false,
      title: "",
    },
    playback: {
      isPlaying: false,
      isBuffering: false,
      downloadSpeedBps: 0,
      currentTime: 0,
      duration: 0,
      bufferedTime: 0,
      videoBitrate: 0,
      hwdecCurrent: "",
      volume: 100,
    },
    window: {
      isFullscreen: false,
    },
  });

  const progressPercent = computed(() => {
    if (state.playback.duration <= 0) return 0;
    return (state.playback.currentTime / state.playback.duration) * 100;
  });

  const bufferedPercent = computed(() => {
    if (state.playback.duration <= 0) return 0;
    return (state.playback.bufferedTime / state.playback.duration) * 100;
  });

  const isUrlModified = computed(() => {
    const nextUrl = state.media.url.trim();
    return Boolean(nextUrl) && nextUrl !== state.media.lastLoadedUrl;
  });

  const commands = usePlaybackCommands(
    state,
    currentWindow,
    coreClient,
    isWindowsPlatform,
  );

  return {
    state,
    progressPercent,
    bufferedPercent,
    isUrlModified,
    formatTime,
    ...commands,
  };
};
