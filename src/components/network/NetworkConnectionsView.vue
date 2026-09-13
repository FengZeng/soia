<script setup lang="ts">
import type { NetworkConnection } from "../../types/network";
import { settingsLocale, translateSettingsText } from "../../i18n";

const props = defineProps<{
    networkConnections: NetworkConnection[];
    selectedConnection: string;
    formatProtocolLabel: (protocol: string) => string;
}>();

const emit = defineEmits<{
    (e: "open-browser", connectionId: string): void;
    (e: "edit", connection: NetworkConnection): void;
    (e: "delete", connection: NetworkConnection): void;
}>();

const tr = (text: string): string =>
    translateSettingsText(settingsLocale.value, text);
</script>

<template>
    <div class="panel__stack">
        <div class="panel__section panel__section--grow">
            <div class="panel__table panel__table--card panel__table--grow">
                <div class="network-connections__list">
                    <div
                        v-for="connection in props.networkConnections"
                        :key="connection.id"
                        class="network-connection-card"
                        :class="{
                            'network-connection-card--active':
                                connection.id === props.selectedConnection,
                        }"
                        role="button"
                        tabindex="0"
                        @click="emit('open-browser', connection.id)"
                        @keydown.enter="emit('open-browser', connection.id)"
                        @keydown.space.prevent="emit('open-browser', connection.id)"
                    >
                        <div class="network-connection-card__title-row">
                            <span class="network-connection-card__badge">
                                {{ props.formatProtocolLabel(connection.protocol) }}
                            </span>
                            <span class="network-connection-card__title">
                                {{ connection.label }}
                            </span>
                        </div>
                        <div class="network-connection-card__sub">
                            {{ connection.baseUrl || tr("Not configured") }}
                        </div>
                        <div class="network-connection-card__meta">
                            {{ tr("User") }}:
                            {{ connection.username ? connection.username : tr("Anonymous") }}
                        </div>
                        <div class="network-connection-card__actions">
                            <button
                                class="network-connection-card__action network-connection-card__action--edit"
                                type="button"
                                :aria-label="tr('Edit connection')"
                                @click.stop="emit('edit', connection)"
                                @keydown.enter.stop
                                @keydown.space.prevent.stop
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path d="M12 20h9" />
                                    <path
                                        d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5Z"
                                    />
                                </svg>
                            </button>
                            <button
                                class="network-connection-card__action network-connection-card__action--remove"
                                type="button"
                                :aria-label="tr('Delete connection')"
                                @click.stop="emit('delete', connection)"
                                @keydown.enter.stop
                                @keydown.space.prevent.stop
                            >
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    viewBox="0 -960 960 960"
                                    fill="currentColor"
                                >
                                    <path
                                        d="M280-120q-33 0-56.5-23.5T200-200v-520h-40v-80h200v-40h240v40h200v80h-40v520q0 33-23.5 56.5T680-120H280Zm400-600H280v520h400v-520ZM360-280h80v-360h-80v360Zm160 0h80v-360h-80v360ZM280-720v520-520Z"
                                    />
                                </svg>
                            </button>
                        </div>
                    </div>
                    <div
                        v-if="!props.networkConnections.length"
                        class="panel__empty"
                    >
                        <div class="panel__empty-title">{{ tr("No connections") }}</div>
                        <div class="panel__empty-body">
                            {{ tr("Click New to add your first WebDAV connection.") }}
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>
