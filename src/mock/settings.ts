import {
    THEME_GRAPHITE_OPTION,
    THEME_OPTIONS,
    THEME_SETTING_GROUP_TITLE,
    THEME_SETTING_LABEL,
} from "../constants/theme";

export type SelectSettingItem = {
    label: string;
    displayLabel?: string;
    value: string;
    type: "select";
    options: string[];
};

export type ToggleSettingItem = {
    label: string;
    displayLabel?: string;
    value: string;
    type: "toggle";
    onValue: string;
    offValue: string;
};

export type SliderSettingItem = {
    label: string;
    displayLabel?: string;
    value: string;
    type: "slider";
    min: number;
    max: number;
    step: number;
    unit: string;
};

export type PathSettingItem = {
    label: string;
    displayLabel?: string;
    value: string;
    type: "path";
    placeholder?: string;
    browseTitle?: string;
    validationMessage?: string;
};

export type TextSettingItem = {
    label: string;
    displayLabel?: string;
    value: string;
    type: "text";
    placeholder?: string;
    validationMessage?: string;
};

export type SettingItem =
    | SelectSettingItem
    | ToggleSettingItem
    | SliderSettingItem
    | PathSettingItem
    | TextSettingItem;
export type SettingGroup = { title: string; items: SettingItem[] };

export const YTDL_PATH_SETTING_LABEL = "SOIA_YTDL_PATH";
export const YTDL_COOKIES_FROM_BROWSER_SETTING_LABEL =
    "SOIA_YTDL_COOKIES_FROM_BROWSER";
export const YTDL_COOKIES_FROM_BROWSER_OPTIONS = [
    "Off",
    "chrome",
    "firefox",
    "edge",
    "opera",
] as const;
export const LOG_PATH_SETTING_LABEL = "SOIA_LOG_PATH";
export const LOG_LEVEL_SETTING_LABEL = "SOIA_LOG_LEVEL";
export const PROXY_MODE_SETTING_LABEL = "SOIA_PROXY_MODE";
export const PROXY_ADDRESS_SETTING_LABEL = "SOIA_PROXY_ADDRESS";
export const SKIP_INTRO_SECONDS_SETTING_LABEL = "SKIP_INTRO_SECONDS";
export const SEEK_STEP_SETTING_LABEL = "Skip Step";
export const ENABLE_COMPACT_MODE_SETTING_LABEL = "ENABLE_COMPACT_MODE";
export const WALLPAPER_MODE_SETTING_LABEL = "WALLPAPER_MODE";
export const AUTO_PLAY_NEXT_IN_PLAYLIST_SETTING_LABEL =
    "AUTO_PLAY_NEXT_IN_PLAYLIST";
export const PLAYBACK_TITLE_SETTING_LABEL = "PLAYBACK_TITLE";
export const ALLOW_URL_INPUT_DURING_PLAYBACK_SETTING_LABEL =
    "ALLOW_URL_INPUT_DURING_PLAYBACK";
export const DEFAULT_SPEED_SETTING_LABEL = "Default Speed";
export const MPV_CONFIG_PATH_SETTING_LABEL = "MPV_CONFIG_PATH";
export const IMAGE_DISPLAY_DURATION_SETTING_LABEL = "IMAGE_DISPLAY_DURATION";
export const DISABLE_SUBTITLES_SETTING_LABEL = "DISABLE_SUBTITLES";
export const OPENSUBTITLES_ENABLED_SETTING_LABEL = "OPENSUBTITLES_ENABLED";
export const OPENSUBTITLES_API_KEY_SETTING_LABEL = "OPENSUBTITLES_API_KEY";
export const OPENSUBTITLES_LANGUAGES_SETTING_LABEL = "OPENSUBTITLES_LANGUAGES";
export const SUBSOURCE_ENABLED_SETTING_LABEL = "SUBSOURCE_ENABLED";
export const SUBSOURCE_API_KEY_SETTING_LABEL = "SUBSOURCE_API_KEY";
export const SUBSOURCE_LANGUAGES_SETTING_LABEL = "SUBSOURCE_LANGUAGES";
export const NETWORK_START_AT_ROOT_SETTING_LABEL = "NETWORK_START_AT_ROOT";
export const YTDL_MAX_RESOLUTION_SETTING_LABEL = "SOIA_YTDL_MAX_RESOLUTION";
export const YTDL_MAX_RESOLUTION_OPTIONS = [
    "4320p",
    "2160p",
    "1440p",
    "1080p",
    "720p",
    "480p",
    "360p",
    "240p",
    "144p",
] as const;
export const NETWORK_PARALLEL_DOWNLOAD_SETTING_LABEL =
    "NETWORK_PARALLEL_DOWNLOAD";
export const ONLINE_SUBTITLES_SETTING_GROUP_TITLE = "Online Subtitles";
export const LANGUAGE_SETTING_LABEL = "LANGUAGE";
export const LANGUAGE_ENGLISH_OPTION = "English";
export const LANGUAGE_SIMPLIFIED_CHINESE_OPTION = "简体中文";
export const LANGUAGE_OPTIONS = [
    LANGUAGE_ENGLISH_OPTION,
    LANGUAGE_SIMPLIFIED_CHINESE_OPTION,
] as const;
export const SETTINGS_UPDATED_EVENT = "soia:settings-updated";

export type PlaybackTitleMode = "Show" | "Editable" | "Hidden";
export const PLAYBACK_TITLE_MODE_OPTIONS: PlaybackTitleMode[] = [
    "Show",
    "Editable",
    "Hidden",
];

export const defaultSettingGroups: SettingGroup[] = [
    {
        title: THEME_SETTING_GROUP_TITLE,
        items: [
            {
                label: THEME_SETTING_LABEL,
                value: THEME_GRAPHITE_OPTION,
                type: "select",
                options: [...THEME_OPTIONS],
            },
            {
                label: LANGUAGE_SETTING_LABEL,
                displayLabel: "Language",
                value: LANGUAGE_OPTIONS[0],
                type: "select",
                options: [...LANGUAGE_OPTIONS],
            },
            {
                label: ENABLE_COMPACT_MODE_SETTING_LABEL,
                displayLabel: "Compact Mode",
                value: "On",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
        ],
    },
    {
        title: "Playback",
        items: [
            {
                label: MPV_CONFIG_PATH_SETTING_LABEL,
                displayLabel: "mpv Config File",
                value: "",
                type: "path",
                placeholder: "Optional mpv.conf file",
                browseTitle: "Select mpv.conf",
            },
            {
                label: PLAYBACK_TITLE_SETTING_LABEL,
                displayLabel: "Playback Title",
                value: "Show",
                type: "select",
                options: [...PLAYBACK_TITLE_MODE_OPTIONS],
            },
            {
                label: AUTO_PLAY_NEXT_IN_PLAYLIST_SETTING_LABEL,
                displayLabel: "Auto-Play Next",
                value: "On",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
            {
                label: DISABLE_SUBTITLES_SETTING_LABEL,
                displayLabel: "Disable Subtitles",
                value: "Off",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
            {
                label: DEFAULT_SPEED_SETTING_LABEL,
                value: "1.0x",
                type: "select",
                options: ["0.5x", "0.75x", "1.0x", "1.25x", "1.5x", "2.0x"],
            },
            {
                label: SEEK_STEP_SETTING_LABEL,
                displayLabel: "Seek Step",
                value: "5",
                type: "slider",
                min: 5,
                max: 100,
                step: 1,
                unit: "s",
            },
            {
                label: SKIP_INTRO_SECONDS_SETTING_LABEL,
                displayLabel: "Skip Intro For New Videos",
                value: "0",
                type: "slider",
                min: 0,
                max: 300,
                step: 1,
                unit: "s",
            },
            {
                label: IMAGE_DISPLAY_DURATION_SETTING_LABEL,
                displayLabel: "Image Display Duration",
                value: "5",
                type: "slider",
                min: 1,
                max: 60,
                step: 1,
                unit: "s",
            },
            // v1: Loop setting is not supported yet. Keep this block for future rollout.
            // {
            //     label: "Loop",
            //     value: "Off",
            //     type: "toggle",
            //     onValue: "On",
            //     offValue: "Off",
            // },
        ],
    },
    {
        title: "Network",
        items: [
            {
                label: NETWORK_START_AT_ROOT_SETTING_LABEL,
                displayLabel: "Always Start at Root",
                value: "Off",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
            {
                label: NETWORK_PARALLEL_DOWNLOAD_SETTING_LABEL,
                displayLabel: "Multi-thread Download",
                value: "Off",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
        ],
    },
    {
        title: ONLINE_SUBTITLES_SETTING_GROUP_TITLE,
        items: [
            {
                label: OPENSUBTITLES_ENABLED_SETTING_LABEL,
                displayLabel: "OpenSubtitles",
                value: "On",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
            {
                label: OPENSUBTITLES_API_KEY_SETTING_LABEL,
                displayLabel: "OpenSubtitles API Key (Optional)",
                value: "",
                type: "text",
                placeholder: "Leave empty to use Soia's shared API key",
            },
            {
                label: OPENSUBTITLES_LANGUAGES_SETTING_LABEL,
                displayLabel: "Subtitle Languages",
                value: "en",
                type: "text",
                placeholder: "en,zh",
            },
            {
                label: SUBSOURCE_ENABLED_SETTING_LABEL,
                displayLabel: "SubSource",
                value: "Off",
                type: "toggle",
                onValue: "On",
                offValue: "Off",
            },
            {
                label: SUBSOURCE_API_KEY_SETTING_LABEL,
                displayLabel: "SubSource API Key",
                value: "",
                type: "text",
                placeholder: "Create a free key from your SubSource profile",
            },
            {
                label: SUBSOURCE_LANGUAGES_SETTING_LABEL,
                displayLabel: "Subtitle Languages",
                value: "english",
                type: "text",
                placeholder: "english,arabic,persian",
            },
        ],
    },
    // v1: Video settings are not supported yet. Keep this block for future rollout.
    // {
    //     title: "Video",
    //     items: [
    //         {
    //             label: "Quality",
    //             value: "Auto (1080p)",
    //             type: "select",
    //             options: [
    //                 "Auto (1080p)",
    //                 "2160p (4K)",
    //                 "1440p",
    //                 "1080p",
    //                 "720p",
    //             ],
    //         },
    //         {
    //             label: "Hardware Decode",
    //             value: "Auto",
    //             type: "select",
    //             options: ["Auto", "On", "Off"],
    //         },
    //         {
    //             label: "Rendering",
    //             value: "GPU",
    //             type: "select",
    //             options: ["GPU", "CPU"],
    //         },
    //         {
    //             label: "HDR",
    //             value: "Off",
    //             type: "select",
    //             options: ["Off", "On", "Auto"],
    //         },
    //     ],
    // },
    // v1: Subtitles settings are not supported yet. Keep this block for future rollout.
    // {
    //     title: "Subtitles",
    //     items: [
    //         {
    //             label: "Default",
    //             value: "Off",
    //             type: "toggle",
    //             onValue: "On",
    //             offValue: "Off",
    //         },
    //         {
    //             label: "Font Size",
    //             value: "16px",
    //             type: "select",
    //             options: ["12px", "14px", "16px", "18px", "20px"],
    //         },
    //         {
    //             label: "Background",
    //             value: "On",
    //             type: "toggle",
    //             onValue: "On",
    //             offValue: "Off",
    //         },
    //         {
    //             label: "Delay",
    //             value: "0 ms",
    //             type: "select",
    //             options: ["-250 ms", "0 ms", "250 ms", "500 ms"],
    //         },
    //     ],
    // },
    // v1: Storage settings are not supported yet. Keep this block for future rollout.
    // {
    //     title: "Storage",
    //     items: [
    //         {
    //             label: "Cache",
    //             value: "50",
    //             type: "slider",
    //             min: 5,
    //             max: 100,
    //             step: 1,
    //             unit: "MB",
    //         },
    //         {
    //             label: "Snapshots",
    //             value: "Enabled",
    //             type: "toggle",
    //             onValue: "Enabled",
    //             offValue: "Disabled",
    //         },
    //     ],
    // },
    {
        title: "Tools",
        items: [
            {
                label: YTDL_PATH_SETTING_LABEL,
                displayLabel: "Video Download Tool (yt-dlp)",
                value: "",
                type: "path",
                placeholder: "Select the yt-dlp executable...",
                browseTitle: "Select yt-dlp executable",
            },
            {
                label: YTDL_COOKIES_FROM_BROWSER_SETTING_LABEL,
                displayLabel: "Browser Cookies",
                value: "Off",
                type: "select",
                options: [...YTDL_COOKIES_FROM_BROWSER_OPTIONS],
            },
            {
                label: YTDL_MAX_RESOLUTION_SETTING_LABEL,
                displayLabel: "Max Stream Resolution",
                value: "1080p",
                type: "select",
                options: [...YTDL_MAX_RESOLUTION_OPTIONS],
            },
            {
                label: PROXY_MODE_SETTING_LABEL,
                displayLabel: "Proxy Type",
                value: "Off",
                type: "select",
                options: ["Off", "HTTP"],
            },
            {
                label: PROXY_ADDRESS_SETTING_LABEL,
                displayLabel: "Proxy Server",
                value: "",
                type: "text",
                placeholder: "127.0.0.1:7890",
            },
            {
                label: LOG_LEVEL_SETTING_LABEL,
                displayLabel: "Log Level",
                value: "Info",
                type: "select",
                options: [
                    "Error",
                    "Warn",
                    "Info",
                    "Debug",
                    "Trace",
                ],
            },
            {
                label: LOG_PATH_SETTING_LABEL,
                displayLabel: "Log File",
                value: "",
                type: "path" as const,
                placeholder: "Log path unavailable",
                browseTitle: "Open log file folder",
            },
        ],
    },
    {
        title: "Experiments",
        items: [
            {
                label: WALLPAPER_MODE_SETTING_LABEL,
                displayLabel: "Wallpaper Mode",
                value: "Disable",
                type: "toggle",
                onValue: "Enable",
                offValue: "Disable",
            },
        ],
    },
];
