import type { AudioDevice } from "../types/audio";

export type AudioDeviceOption = {
    value: string;
    label: string;
    unavailable: boolean;
};

export const createAudioDeviceOptions = (
    devices: readonly AudioDevice[],
    selectedDevice = "auto",
): AudioDeviceOption[] => {
    const visible = devices.filter((device) => device.name !== "auto");
    const totals = new Map<string, number>();
    visible.forEach((device) => {
        totals.set(device.description, (totals.get(device.description) ?? 0) + 1);
    });

    const occurrences = new Map<string, number>();
    const options: AudioDeviceOption[] = [
        { value: "auto", label: "Automatic", unavailable: false },
    ];
    visible.forEach((device) => {
        const occurrence = (occurrences.get(device.description) ?? 0) + 1;
        occurrences.set(device.description, occurrence);
        options.push({
            value: device.name,
            label:
                (totals.get(device.description) ?? 0) > 1
                    ? `${device.description} (${occurrence})`
                    : device.description,
            unavailable: false,
        });
    });

    if (
        selectedDevice !== "auto" &&
        !options.some((option) => option.value === selectedDevice)
    ) {
        options.push({
            value: selectedDevice,
            label: "Unavailable output",
            unavailable: true,
        });
    }

    return options;
};
