<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import type { RemoteControlInfo } from "../composables/useRemoteControlQrDialog";

defineProps<{
    open: boolean;
    info: RemoteControlInfo | null;
    secondsRemaining: number;
}>();

const emit = defineEmits<{
    (e: "close"): void;
}>();

const onKeydown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
        emit("close");
    }
};

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
    <Teleport to="body">
        <Transition name="remote-control-qr-dialog">
            <div
                v-if="open && info"
                class="remote-control-qr-dialog__backdrop"
                @mousedown.self="emit('close')"
            >
                <section
                    class="remote-control-qr-dialog"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="remote-control-qr-dialog-title"
                >
                    <button
                        class="remote-control-qr-dialog__close"
                        type="button"
                        aria-label="Close QR code"
                        @click="emit('close')"
                    >
                        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
                    </button>
                    <div class="remote-control-qr-dialog__heading">
                        <span class="remote-control-qr-dialog__eyebrow">Remote Controller</span>
                        <h2 id="remote-control-qr-dialog-title">Scan to connect</h2>
                        <p>Use a phone on the same local network.</p>
                    </div>
                    <div class="remote-control-qr-dialog__code" v-html="info.qrSvg"></div>
                    <div class="remote-control-qr-dialog__expiry">
                        <span class="remote-control-qr-dialog__timer">{{ secondsRemaining }}</span>
                        <span>seconds remaining</span>
                    </div>
                </section>
            </div>
        </Transition>
    </Teleport>
</template>
