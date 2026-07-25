import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, ref } from "vue";

export type RemoteControlInfo = { url: string; qrSvg: string };
export type RemoteControlStatus = { enabled: boolean; connectedDevices: number };

const REMOTE_QR_TTL_MS = 99_000;

export const useRemoteControlQrDialog = (
    onStatusChange?: (status: RemoteControlStatus) => void,
) => {
    const remoteControlInfo = ref<RemoteControlInfo | null>(null);
    const isRemoteQrOpen = ref(false);
    const remoteQrSecondsRemaining = ref(99);
    let remoteQrMonitorTimer: number | null = null;
    let remoteQrExpiresAt = 0;
    let remoteQrInitialDeviceCount = 0;
    let isRemoteQrStatusRequestPending = false;

    const getRemoteControlStatus = () =>
        invoke<RemoteControlStatus>("get_remote_control_status");

    const closeRemoteQrDialog = () => {
        isRemoteQrOpen.value = false;
        remoteControlInfo.value = null;
        if (remoteQrMonitorTimer !== null) {
            window.clearInterval(remoteQrMonitorTimer);
            remoteQrMonitorTimer = null;
        }
    };

    const monitorRemoteQr = () => {
        remoteQrExpiresAt = Date.now() + REMOTE_QR_TTL_MS;
        remoteQrSecondsRemaining.value = REMOTE_QR_TTL_MS / 1_000;
        remoteQrMonitorTimer = window.setInterval(async () => {
            const remaining = Math.max(0, Math.ceil((remoteQrExpiresAt - Date.now()) / 1000));
            remoteQrSecondsRemaining.value = remaining;
            if (remaining <= 0) {
                closeRemoteQrDialog();
                return;
            }
            if (isRemoteQrStatusRequestPending) return;
            isRemoteQrStatusRequestPending = true;
            try {
                const status = await getRemoteControlStatus();
                onStatusChange?.(status);
                if (status.connectedDevices > remoteQrInitialDeviceCount) {
                    closeRemoteQrDialog();
                }
            } catch {
                // Keep the QR visible until it expires if status polling briefly fails.
            } finally {
                isRemoteQrStatusRequestPending = false;
            }
        }, 500);
    };

    const showRemoteControlQr = async () => {
        closeRemoteQrDialog();
        const status = await getRemoteControlStatus();
        onStatusChange?.(status);
        remoteQrInitialDeviceCount = status.connectedDevices;
        remoteControlInfo.value = await invoke<RemoteControlInfo>("get_remote_control_info");
        isRemoteQrOpen.value = true;
        monitorRemoteQr();
    };

    onBeforeUnmount(closeRemoteQrDialog);

    return {
        remoteControlInfo,
        isRemoteQrOpen,
        remoteQrSecondsRemaining,
        getRemoteControlStatus,
        showRemoteControlQr,
        closeRemoteQrDialog,
    };
};
