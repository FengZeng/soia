import { computed, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { MediaTrack } from "../types/media";
import type { CoreClient } from "../core-client/CoreClient";

export type SubtitleTarget = "primary" | "secondary";

type SelectSubtitlePayload = {
    target: SubtitleTarget;
    track: MediaTrack;
};

const isSameTrackId = (left: MediaTrack["id"], right: MediaTrack["id"]) =>
    String(left) === String(right);

export const useSubtitleState = (
    subTracks: Ref<MediaTrack[]>,
    coreClient: CoreClient,
) => {
    const dualSubEnabled = ref(false);
    const secondarySubId = ref<MediaTrack["id"]>(0);
    const activeSubTarget = ref<SubtitleTarget>("primary");

    const primarySubId = computed<MediaTrack["id"]>(
        () => subTracks.value.find((track) => track.selected)?.id ?? 0,
    );

    const setSecondarySid = async (value: MediaTrack["id"]) => {
        secondarySubId.value = value;
        await invoke("mpv_set_option_string", {
            name: "secondary-sid",
            value: value === 0 ? "no" : value,
        });
    };

    const clearSecondarySid = async () => {
        await setSecondarySid(0);
    };

    const setDualSubEnabled = async (enabled: boolean) => {
        dualSubEnabled.value = enabled;
        if (enabled) return;
        activeSubTarget.value = "primary";
        await clearSecondarySid();
    };

    const setActiveSubTarget = (target: SubtitleTarget) => {
        if (!dualSubEnabled.value && target === "secondary") {
            activeSubTarget.value = "primary";
            return;
        }
        activeSubTarget.value = target;
    };

    const selectPrimarySub = async (track: MediaTrack) => {
        if (track.id !== 0 && isSameTrackId(track.id, secondarySubId.value)) {
            await clearSecondarySid();
        }
        if (track.id === 0) {
            await coreClient.execute({ type: "disableSubtitles" });
            return;
        }
        await coreClient.execute({
            type: "selectSubtitleTrack",
            trackId: Number(track.id),
        });
    };

    const selectSecondarySub = async (track: MediaTrack) => {
        if (track.id !== 0 && isSameTrackId(track.id, primarySubId.value)) {
            await clearSecondarySid();
            return;
        }
        if (!dualSubEnabled.value && track.id !== 0) {
            dualSubEnabled.value = true;
        }
        await setSecondarySid(track.id);
    };

    const selectSubTrack = async ({ target, track }: SelectSubtitlePayload) => {
        if (!dualSubEnabled.value || target === "primary") {
            await selectPrimarySub(track);
            return;
        }
        await selectSecondarySub(track);
    };

    const reconcileSubtitleState = async (tracks: MediaTrack[]) => {
        const primarySelectedId = tracks.find((t) => t.selected)?.id ?? 0;
        const hasSecondaryTrack = tracks.some((t) =>
            isSameTrackId(t.id, secondarySubId.value),
        );
        if (secondarySubId.value !== 0 && !hasSecondaryTrack) {
            await clearSecondarySid();
        }
        if (
            secondarySubId.value !== 0 &&
            isSameTrackId(secondarySubId.value, primarySelectedId)
        ) {
            await clearSecondarySid();
        }
        if (!tracks.length) {
            dualSubEnabled.value = false;
            activeSubTarget.value = "primary";
            if (secondarySubId.value !== 0) {
                await clearSecondarySid();
            }
        }
    };

    const disableAllSubtitles = async () => {
        dualSubEnabled.value = false;
        activeSubTarget.value = "primary";
        secondarySubId.value = 0;
        await invoke("mpv_set_option_string", { name: "secondary-sid", value: "no" });
        await coreClient.execute({ type: "disableSubtitles" });
    };

    const enableAutoSubtitleSelection = async () => {
        await invoke("mpv_set_option_string", { name: "sid", value: "auto" });
    };

    const resetSubtitleState = () => {
        dualSubEnabled.value = false;
        secondarySubId.value = 0;
        activeSubTarget.value = "primary";
    };

    return {
        dualSubEnabled,
        secondarySubId,
        activeSubTarget,
        primarySubId,
        setDualSubEnabled,
        setActiveSubTarget,
        selectSubTrack,
        reconcileSubtitleState,
        disableAllSubtitles,
        enableAutoSubtitleSelection,
        resetSubtitleState,
    };
};
