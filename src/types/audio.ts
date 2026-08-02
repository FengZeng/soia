export type AudioMode = "pcm" | "passthrough";

export type AudioSettings = {
    schemaVersion: number;
    mode: AudioMode;
    outputDevice: string;
};

export type AudioDevice = {
    name: string;
    description: string;
};

export type ActiveAudioMode =
    | "passthrough"
    | "decoded_pcm"
    | "null_output";

export type AudioOutputIssue =
    | "speed_incompatible"
    | "device_disconnected"
    | "passthrough_open_failed"
    | "output_unavailable";

export type AudioOutputStatus = {
    requestedMode: AudioMode;
    activeMode: ActiveAudioMode;
    inputCodec: string | null;
    inputProfile: string | null;
    inputChannels: string | null;
    selectedDevice: string;
    currentAo: string | null;
    decoder: string | null;
    outputFormat: string | null;
    outputRate: number | null;
    outputChannels: string | null;
    passthroughActive: boolean;
    outputIssue: AudioOutputIssue | null;
};

export const defaultAudioSettings = (): AudioSettings => ({
    schemaVersion: 1,
    mode: "pcm",
    outputDevice: "auto",
});

export const defaultAudioOutputStatus = (): AudioOutputStatus => ({
    requestedMode: "pcm",
    activeMode: "null_output",
    inputCodec: null,
    inputProfile: null,
    inputChannels: null,
    selectedDevice: "auto",
    currentAo: null,
    decoder: null,
    outputFormat: null,
    outputRate: null,
    outputChannels: null,
    passthroughActive: false,
    outputIssue: null,
});
