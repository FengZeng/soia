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
    const uniqueByDescription = new Map<string, AudioDevice>();
    visible.forEach((device) => {
        const existing = uniqueByDescription.get(device.description);
        if (!existing || device.name === selectedDevice) {
            uniqueByDescription.set(device.description, device);
        }
    });
    const uniqueVisible = [...uniqueByDescription.values()];

    const options: AudioDeviceOption[] = [
        { value: "auto", label: "Automatic", unavailable: false },
    ];
    uniqueVisible.forEach((device) => {
        options.push({
            value: device.name,
            label: device.description,
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
