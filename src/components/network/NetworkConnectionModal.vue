<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { settingsLocale, translateSettingsText } from "../../i18n";

type ProtocolOption = {
    value: string;
    label: string;
};

type ConnectionForm = {
    label: string;
    protocol: string;
    baseUrl: string;
    host: string;
    share: string;
    group: string;
    port: string;
    username: string;
    password: string;
    defaultPath: string;
};

const props = defineProps<{
    open: boolean;
    isEditingConnection: boolean;
    selectedProtocolLabel: string;
    protocolOptions: ProtocolOption[];
    createForm: ConnectionForm;
    isSmbProtocol: boolean;
    isFtpProtocol: boolean;
    requiresAuthFields: boolean;
    serverFieldLabel: string;
    serverFieldPlaceholder: string;
    defaultPathLabel: string;
    createError: string;
    isCreatingConnection: boolean;
}>();

const tr = (text: string): string =>
    translateSettingsText(settingsLocale.value, text);

const emit = defineEmits<{
    (e: "close"): void;
    (e: "submit"): void;
}>();

const isProtocolSelectOpen = ref(false);
const activeProtocolOptionIndex = ref(0);
const protocolSelectTrigger = ref<HTMLElement | null>(null);
const protocolSelectMenuStyle = ref<Record<string, string>>({});

const selectedProtocolOption = computed(
    () =>
        props.protocolOptions.find(
            (option) => option.value === props.createForm.protocol,
        ) ?? props.protocolOptions[0],
);

const selectedProtocolOptionLabel = computed(
    () => tr(selectedProtocolOption.value?.label ?? props.selectedProtocolLabel),
);

const getProtocolOptionIndex = () => {
    const index = props.protocolOptions.findIndex(
        (option) => option.value === props.createForm.protocol,
    );
    return index >= 0 ? index : 0;
};

const clampProtocolOptionIndex = (index: number): number => {
    if (!props.protocolOptions.length) return 0;
    if (index < 0) return props.protocolOptions.length - 1;
    if (index >= props.protocolOptions.length) return 0;
    return index;
};

const updateProtocolSelectMenuPosition = () => {
    if (!isProtocolSelectOpen.value || !protocolSelectTrigger.value) return;

    const trigger = protocolSelectTrigger.value;
    const rect = trigger.getBoundingClientRect();
    const viewportHeight = window.innerHeight;
    const spaceAbove = rect.top;
    const spaceBelow = viewportHeight - rect.bottom;
    const estimatedMenuHeight = 240;
    const shouldOpenTop =
        spaceBelow < estimatedMenuHeight && spaceAbove > spaceBelow;
    const gap = 6;
    const maxHeight = Math.max(
        120,
        Math.min(320, shouldOpenTop ? spaceAbove - 10 : spaceBelow - 10),
    );
    const triggerStyles = getComputedStyle(trigger);
    const menuThemeVars: Record<string, string> = {
        "--panel-select-card-border": triggerStyles
            .getPropertyValue("--panel-select-card-border")
            .trim(),
        "--panel-select-card-text": triggerStyles
            .getPropertyValue("--panel-select-card-text")
            .trim(),
        "--panel-select-card-hover-bg": triggerStyles
            .getPropertyValue("--panel-select-card-hover-bg")
            .trim(),
        "--panel-select-card-focus-bg": triggerStyles
            .getPropertyValue("--panel-select-card-focus-bg")
            .trim(),
        "--panel-select-card-focus-border": triggerStyles
            .getPropertyValue("--panel-select-card-focus-border")
            .trim(),
        "--panel-select-menu-bg": triggerStyles
            .getPropertyValue("--panel-select-menu-bg")
            .trim(),
        "--panel-select-menu-border": triggerStyles
            .getPropertyValue("--panel-select-menu-border")
            .trim(),
        "--panel-select-menu-hover-bg": triggerStyles
            .getPropertyValue("--panel-select-menu-hover-bg")
            .trim(),
        "--panel-select-menu-selected-bg": triggerStyles
            .getPropertyValue("--panel-select-menu-selected-bg")
            .trim(),
        "--panel-select-menu-selected-border": triggerStyles
            .getPropertyValue("--panel-select-menu-selected-border")
            .trim(),
    };

    protocolSelectMenuStyle.value = shouldOpenTop
        ? {
              ...menuThemeVars,
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              bottom: `${viewportHeight - rect.top + gap}px`,
              maxHeight: `${maxHeight}px`,
          }
        : {
              ...menuThemeVars,
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              top: `${rect.bottom + gap}px`,
              maxHeight: `${maxHeight}px`,
          };
};

const openProtocolSelect = () => {
    isProtocolSelectOpen.value = true;
    activeProtocolOptionIndex.value = getProtocolOptionIndex();
    nextTick(() => {
        updateProtocolSelectMenuPosition();
    });
};

const closeProtocolSelect = () => {
    isProtocolSelectOpen.value = false;
};

const toggleProtocolSelect = () => {
    if (isProtocolSelectOpen.value) {
        closeProtocolSelect();
        return;
    }
    openProtocolSelect();
};

const chooseProtocolOption = (option: ProtocolOption) => {
    props.createForm.protocol = option.value;
    closeProtocolSelect();
};

const setActiveProtocolOption = (step: number) => {
    activeProtocolOptionIndex.value = clampProtocolOptionIndex(
        activeProtocolOptionIndex.value + step,
    );
};

const onProtocolSelectKeydown = (event: KeyboardEvent) => {
    if (!props.protocolOptions.length) return;

    if (event.key === "ArrowDown") {
        event.preventDefault();
        if (!isProtocolSelectOpen.value) {
            openProtocolSelect();
            return;
        }
        setActiveProtocolOption(1);
        return;
    }

    if (event.key === "ArrowUp") {
        event.preventDefault();
        if (!isProtocolSelectOpen.value) {
            openProtocolSelect();
            return;
        }
        setActiveProtocolOption(-1);
        return;
    }

    if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        if (!isProtocolSelectOpen.value) {
            openProtocolSelect();
            return;
        }
        const nextOption = props.protocolOptions[activeProtocolOptionIndex.value];
        if (nextOption) {
            chooseProtocolOption(nextOption);
        }
        return;
    }

    if (event.key === "Escape" && isProtocolSelectOpen.value) {
        event.preventDefault();
        closeProtocolSelect();
    }
};

const onDocumentPointerDown = (event: PointerEvent) => {
    if (!isProtocolSelectOpen.value) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".panel__custom-select, .panel__custom-select-menu")) {
        return;
    }
    closeProtocolSelect();
};

watch(
    () => props.open,
    (open) => {
        if (!open) closeProtocolSelect();
    },
);

onMounted(() => {
    document.addEventListener("pointerdown", onDocumentPointerDown);
    document.addEventListener("scroll", updateProtocolSelectMenuPosition, true);
    window.addEventListener("resize", updateProtocolSelectMenuPosition);
});

onBeforeUnmount(() => {
    document.removeEventListener("pointerdown", onDocumentPointerDown);
    document.removeEventListener("scroll", updateProtocolSelectMenuPosition, true);
    window.removeEventListener("resize", updateProtocolSelectMenuPosition);
});
</script>

<template>
    <div
        v-if="props.open"
        class="network-modal__backdrop"
        @click="emit('close')"
    ></div>
    <div v-if="props.open" class="network-modal ui-surface">
        <div class="network-modal__title">
            {{
                props.isEditingConnection
                ? `${tr("Edit")} ${props.selectedProtocolLabel} ${tr("Connection")}`
                : `${tr("Add")} ${props.selectedProtocolLabel} ${tr("Connection")}`
            }}
        </div>
        <div class="network-modal__form">
            <label class="network-modal__field">
                <span>{{ tr("Protocol") }}</span>
                <div
                    class="panel__custom-select"
                    :class="{
                        'panel__custom-select--open': isProtocolSelectOpen,
                    }"
                >
                    <button
                        ref="protocolSelectTrigger"
                        type="button"
                        class="panel__custom-select-trigger network-modal__input network-modal__custom-select-trigger"
                        :aria-expanded="isProtocolSelectOpen"
                        aria-haspopup="listbox"
                        @click="toggleProtocolSelect"
                        @keydown="onProtocolSelectKeydown"
                    >
                        <span class="panel__custom-select-value">
                            {{ selectedProtocolOptionLabel }}
                        </span>
                        <span class="panel__custom-select-arrow" aria-hidden="true">
                            <svg viewBox="0 0 12 12">
                                <path d="M2.25 4.5L6 8.25L9.75 4.5" />
                            </svg>
                        </span>
                    </button>
                    <Teleport to="body">
                        <div
                            v-if="isProtocolSelectOpen"
                            class="panel__custom-select-menu"
                            :style="protocolSelectMenuStyle"
                            role="listbox"
                            :aria-label="tr('Protocol')"
                        >
                            <button
                                v-for="(option, optionIndex) in props.protocolOptions"
                                :key="option.value"
                                type="button"
                                class="panel__custom-select-option"
                                :class="{
                                    'panel__custom-select-option--selected':
                                        option.value === props.createForm.protocol,
                                    'panel__custom-select-option--active':
                                        optionIndex === activeProtocolOptionIndex,
                                }"
                                role="option"
                                :aria-selected="option.value === props.createForm.protocol"
                                @mouseenter="activeProtocolOptionIndex = optionIndex"
                                @click="chooseProtocolOption(option)"
                            >
                                {{ option.label }}
                            </button>
                        </div>
                    </Teleport>
                </div>
            </label>
            <label class="network-modal__field">
                <span>{{ tr("Name") }}</span>
                <input
                    v-model="props.createForm.label"
                    class="panel__input panel__input--path network-modal__input"
                    type="text"
                    :placeholder="tr('Optional (auto fill)')"
                />
            </label>

            <template v-if="props.isSmbProtocol">
                <label class="network-modal__field">
                    <span>{{ tr("Host") }}</span>
                    <input
                        v-model="props.createForm.host"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        placeholder="192.168.31.33"
                    />
                </label>
                <label class="network-modal__field">
                    <span>{{ tr("Share (optional)") }}</span>
                    <input
                        v-model="props.createForm.share"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        :placeholder="tr('Leave empty to browse shares')"
                    />
                </label>
            </template>

            <template v-else-if="props.isFtpProtocol">
                <label class="network-modal__field">
                    <span>{{ tr("Host") }}</span>
                    <input
                        v-model="props.createForm.host"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        placeholder="192.168.31.50"
                    />
                </label>
                <label class="network-modal__field">
                    <span>{{ tr("Port") }}</span>
                    <input
                        v-model="props.createForm.port"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        inputmode="numeric"
                        placeholder="21"
                    />
                </label>
            </template>

            <template v-else>
                <label class="network-modal__field">
                    <span>{{ props.serverFieldLabel }}</span>
                    <input
                        v-model="props.createForm.baseUrl"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        :placeholder="props.serverFieldPlaceholder"
                    />
                </label>
            </template>

            <template v-if="props.requiresAuthFields">
                <label class="network-modal__field">
                    <span>{{ tr("Username") }}</span>
                    <input
                        v-model="props.createForm.username"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        :placeholder="tr('Optional')"
                    />
                </label>
                <label class="network-modal__field">
                    <span>{{ tr("Password") }}</span>
                    <input
                        v-model="props.createForm.password"
                        class="panel__input panel__input--path network-modal__input"
                        type="password"
                        :placeholder="tr('Optional')"
                    />
                </label>
                <label
                    v-if="props.isSmbProtocol"
                    class="network-modal__field"
                >
                    <span>{{ tr("Group") }}</span>
                    <input
                        v-model="props.createForm.group"
                        class="panel__input panel__input--path network-modal__input"
                        type="text"
                        :placeholder="tr('Optional')"
                    />
                </label>
            </template>
            <label class="network-modal__field">
                <span>{{ props.defaultPathLabel }}</span>
                <input
                    v-model="props.createForm.defaultPath"
                    class="panel__input panel__input--path network-modal__input"
                    type="text"
                    placeholder="/"
                />
            </label>
        </div>
        <div v-if="props.createError" class="network-modal__error">
            {{ props.createError }}
        </div>
        <div class="network-modal__actions">
            <button
                class="panel__action panel__action--ghost network-modal__btn"
                type="button"
                :disabled="props.isCreatingConnection"
                @click="emit('close')"
            >
                {{ tr("Cancel") }}
            </button>
            <button
                class="panel__action network-modal__btn"
                type="button"
                :disabled="props.isCreatingConnection"
                @click="emit('submit')"
            >
                {{
                    props.isCreatingConnection
                        ? tr("Saving...")
                        : props.isEditingConnection
                          ? tr("Save")
                          : tr("Create")
                }}
            </button>
        </div>
    </div>
</template>
