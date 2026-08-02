import { ref, watch } from "vue";
import type { SettingGroup, SettingItem } from "../../mock/settings";
import {
    defaultAudioSettings,
    type AudioDevice,
    type AudioSettings,
} from "../../types/audio";
import { createAudioDeviceOptions } from "../../utils/audioDevices";
import { useAudioOutput } from "../useAudioOutput";

export const AUDIO_GROUP_TITLE = "Audio";

const OUTPUT_LABEL = "AUDIO_OUTPUT_DEVICE";
const PASSTHROUGH_LABEL = "AUDIO_PASSTHROUGH";

const itemValue = (group: SettingGroup, label: string) =>
    group.items.find((item) => item.label === label)?.value ?? "";

const selectItem = (
    label: string,
    displayLabel: string,
    value: string,
    options: string[],
): SettingItem => ({
    label,
    displayLabel,
    value,
    type: "select",
    options,
});

const toggleItem = (
    label: string,
    displayLabel: string,
    enabled: boolean,
): SettingItem => ({
    label,
    displayLabel,
    value: enabled ? "On" : "Off",
    type: "toggle",
    onValue: "On",
    offValue: "Off",
});

export const useAudioSettingsSection = () => {
    const output = useAudioOutput();
    const group = ref<SettingGroup>({
        title: AUDIO_GROUP_TITLE,
        items: [],
    });
    let currentSettings = defaultAudioSettings();
    let deviceNameByLabel = new Map<string, string>();
    let deviceLabelByName = new Map<string, string>();
    let lastApplied = "";

    const rebuild = (settings: AudioSettings, devices: readonly AudioDevice[]) => {
        currentSettings = {
            ...settings,
        };

        const options = createAudioDeviceOptions(devices, settings.outputDevice);
        deviceNameByLabel = new Map(
            options.map((device) => [device.label, device.value]),
        );
        deviceLabelByName = new Map(
            options.map((device) => [device.value, device.label]),
        );
        const selectedDevice =
            deviceLabelByName.get(settings.outputDevice) ?? "Automatic";
        const outputOptions = options.map((device) => device.label);

        group.value = {
            title: AUDIO_GROUP_TITLE,
            items: [
                selectItem(
                    OUTPUT_LABEL,
                    "Output",
                    selectedDevice,
                    outputOptions,
                ),
                toggleItem(
                    PASSTHROUGH_LABEL,
                    "Passthrough",
                    settings.mode === "passthrough",
                ),
            ],
        };
    };

    const toPersistedAudio = (): AudioSettings => {
        const selectedLabel = itemValue(group.value, OUTPUT_LABEL);
        const selectedDevice =
            selectedLabel === "Automatic"
                ? "auto"
                : deviceNameByLabel.get(selectedLabel) ?? "auto";
        const passthrough = itemValue(group.value, PASSTHROUGH_LABEL) === "On";

        return {
            ...currentSettings,
            schemaVersion: 1,
            mode: passthrough ? "passthrough" : "pcm",
            outputDevice: selectedDevice,
        };
    };

    const loadAudioSettings = async (_stored?: AudioSettings) => {
        await output.refresh();
        const resolved = output.settings.value;
        rebuild(resolved, output.devices.value);
        lastApplied = JSON.stringify(resolved);
    };

    const applySectionSideEffects = async () => {
        const requested = toPersistedAudio();
        const serialized = JSON.stringify(requested);
        if (serialized === lastApplied) return;
        lastApplied = serialized;
        try {
            const applied = await output.applySettings(requested);
            lastApplied = JSON.stringify(applied);
            rebuild(applied, output.devices.value);
        } catch {
            lastApplied = "";
        }
    };

    const resetAudioSettings = () => {
        const defaults = defaultAudioSettings();
        lastApplied = "";
        rebuild(defaults, output.devices.value);
    };

    watch(
        output.devices,
        (nextDevices) => {
            if (!group.value.items.length) return;
            rebuild(toPersistedAudio(), nextDevices);
        },
        { deep: true },
    );

    return {
        settingGroup: group,
        outputStatus: output.status,
        outputError: output.error,
        retryOutput: output.retryOutput,
        loadAudioSettings,
        applySectionSideEffects,
        resetAudioSettings,
        toPersistedAudio,
    };
};
