import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ref } from "vue";

const PROJECT_GITHUB_URL = "https://github.com/FengZeng/soia";

export type RuntimeVersions = {
    soiaVersion: string;
    mpvVersion?: string | null;
    ffmpegVersion?: string | null;
};

export const useAboutSection = () => {
    const runtimeVersions = ref<RuntimeVersions | null>(null);

    const loadRuntimeVersions = async () => {
        try {
            runtimeVersions.value = await invoke<RuntimeVersions>(
                "get_runtime_versions",
            );
        } catch {
            runtimeVersions.value = null;
        }
    };

    const openProjectGithub = async () => {
        try {
            await openUrl(PROJECT_GITHUB_URL);
            return;
        } catch {
            if (typeof window !== "undefined") {
                window.open(PROJECT_GITHUB_URL, "_blank", "noopener,noreferrer");
            }
        }
    };

    return {
        runtimeVersions,
        loadRuntimeVersions,
        openProjectGithub,
    };
};
