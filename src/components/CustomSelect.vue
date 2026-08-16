<script setup lang="ts">
import {
    computed,
    nextTick,
    onBeforeUnmount,
    onMounted,
    ref,
    watch,
} from "vue";

type SelectOption = {
    value: string;
    label: string;
};

const props = defineProps<{
    modelValue: string;
    options: readonly (string | SelectOption)[];
    ariaLabel: string;
}>();

const emit = defineEmits<{
    (event: "update:modelValue", value: string): void;
}>();

const isOpen = ref(false);
const activeOptionIndex = ref(0);
const trigger = ref<HTMLElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const menuStyle = ref<Record<string, string>>({});

const normalizedOptions = computed<SelectOption[]>(() =>
    props.options.map((option) =>
        typeof option === "string"
            ? { value: option, label: option }
            : option,
    ),
);

const selectedOption = computed(
    () =>
        normalizedOptions.value.find(
            (option) => option.value === props.modelValue,
        ) ?? normalizedOptions.value[0],
);

const selectedOptionIndex = () => {
    const index = normalizedOptions.value.findIndex(
        (option) => option.value === props.modelValue,
    );
    return index >= 0 ? index : 0;
};

const clampOptionIndex = (index: number) => {
    if (!normalizedOptions.value.length) return 0;
    if (index < 0) return normalizedOptions.value.length - 1;
    if (index >= normalizedOptions.value.length) return 0;
    return index;
};

const updateMenuPosition = () => {
    if (!isOpen.value || !trigger.value) return;

    const rect = trigger.value.getBoundingClientRect();
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
    const triggerStyles = getComputedStyle(trigger.value);
    const themeVariables = [
        "--panel-select-card-border",
        "--panel-select-card-text",
        "--panel-select-card-hover-bg",
        "--panel-select-card-focus-bg",
        "--panel-select-card-focus-border",
        "--panel-select-menu-bg",
        "--panel-select-menu-border",
        "--panel-select-menu-hover-bg",
        "--panel-select-menu-selected-bg",
        "--panel-select-menu-selected-border",
    ].reduce<Record<string, string>>((variables, name) => {
        variables[name] = triggerStyles.getPropertyValue(name).trim();
        return variables;
    }, {});

    menuStyle.value = shouldOpenTop
        ? {
              ...themeVariables,
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              bottom: `${viewportHeight - rect.top + gap}px`,
              maxHeight: `${maxHeight}px`,
          }
        : {
              ...themeVariables,
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              top: `${rect.bottom + gap}px`,
              maxHeight: `${maxHeight}px`,
          };
};

const open = () => {
    if (!normalizedOptions.value.length) return;
    isOpen.value = true;
    activeOptionIndex.value = selectedOptionIndex();
    nextTick(updateMenuPosition);
};

const close = () => {
    isOpen.value = false;
};

const toggle = () => {
    if (isOpen.value) {
        close();
        return;
    }
    open();
};

const choose = (option: SelectOption) => {
    emit("update:modelValue", option.value);
    close();
};

const onKeydown = (event: KeyboardEvent) => {
    if (!normalizedOptions.value.length) return;

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        if (!isOpen.value) {
            open();
            return;
        }
        activeOptionIndex.value = clampOptionIndex(
            activeOptionIndex.value + (event.key === "ArrowDown" ? 1 : -1),
        );
        return;
    }

    if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        if (!isOpen.value) {
            open();
            return;
        }
        const option = normalizedOptions.value[activeOptionIndex.value];
        if (option) choose(option);
        return;
    }

    if (event.key === "Escape" && isOpen.value) {
        event.preventDefault();
        close();
    }
};

const onDocumentPointerDown = (event: PointerEvent) => {
    if (!isOpen.value) return;
    const target = event.target as Node | null;
    if (
        target &&
        (trigger.value?.contains(target) || menu.value?.contains(target))
    ) {
        return;
    }
    close();
};

watch(
    () => props.modelValue,
    () => {
        if (isOpen.value) activeOptionIndex.value = selectedOptionIndex();
    },
);

watch(
    () => normalizedOptions.value.length,
    (optionCount) => {
        if (!optionCount) close();
    },
);

onMounted(() => {
    document.addEventListener("pointerdown", onDocumentPointerDown);
    document.addEventListener("scroll", updateMenuPosition, true);
    window.addEventListener("resize", updateMenuPosition);
});

onBeforeUnmount(() => {
    document.removeEventListener("pointerdown", onDocumentPointerDown);
    document.removeEventListener("scroll", updateMenuPosition, true);
    window.removeEventListener("resize", updateMenuPosition);
});
</script>

<template>
    <div
        class="panel__custom-select"
        :class="{ 'panel__custom-select--open': isOpen }"
    >
        <button
            ref="trigger"
            class="panel__custom-select-trigger"
            type="button"
            :aria-label="ariaLabel"
            :aria-expanded="isOpen"
            aria-haspopup="listbox"
            :disabled="!normalizedOptions.length"
            @click.stop="toggle"
            @keydown="onKeydown"
        >
            <span class="panel__custom-select-value">
                {{ selectedOption?.label ?? "" }}
            </span>
            <span class="panel__custom-select-arrow" aria-hidden="true">
                <svg viewBox="0 0 12 12">
                    <path d="M2.25 4.5L6 8.25L9.75 4.5" />
                </svg>
            </span>
        </button>
        <Teleport to="body">
            <div
                v-if="isOpen"
                ref="menu"
                class="panel__custom-select-menu"
                :style="menuStyle"
                role="listbox"
                :aria-label="ariaLabel"
            >
                <button
                    v-for="(option, optionIndex) in normalizedOptions"
                    :key="option.value"
                    class="panel__custom-select-option"
                    :class="{
                        'panel__custom-select-option--selected':
                            option.value === modelValue,
                        'panel__custom-select-option--active':
                            optionIndex === activeOptionIndex,
                    }"
                    type="button"
                    role="option"
                    :aria-selected="option.value === modelValue"
                    @mouseenter="activeOptionIndex = optionIndex"
                    @click="choose(option)"
                >
                    {{ option.label }}
                </button>
            </div>
        </Teleport>
    </div>
</template>

<style scoped>
.panel__custom-select {
    position: relative;
    width: 100%;
}

.panel__custom-select-trigger {
    width: 100%;
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px 7px 12px;
    border-radius: 12px;
    border: 1px solid var(--panel-select-card-border);
    background: var(--panel-select-card-bg);
    color: var(--panel-select-card-text);
    font: inherit;
    font-size: 12px;
    line-height: 1.2;
    text-align: left;
    cursor: pointer;
    transition:
        background-color 0.18s ease,
        border-color 0.18s ease,
        box-shadow 0.18s ease;
}

.panel__custom-select-trigger:hover {
    border-color: var(--panel-select-card-hover-border);
    background: var(--panel-select-card-hover-bg);
}

.panel__custom-select-trigger:focus-visible {
    outline: none;
    border-color: var(--panel-select-card-focus-border);
    box-shadow: 0 0 0 2px var(--panel-select-card-focus-glow);
}

.panel__custom-select-trigger:disabled {
    cursor: default;
    opacity: 0.55;
}

.panel__custom-select-value {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.panel__custom-select-arrow {
    width: 14px;
    height: 14px;
    flex: none;
    color: var(--panel-select-card-arrow);
    opacity: 0.94;
    transition: transform 0.18s ease;
}

.panel__custom-select-arrow svg {
    width: 100%;
    height: 100%;
    display: block;
    stroke: currentColor;
    fill: none;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
}

.panel__custom-select--open .panel__custom-select-arrow {
    transform: rotate(180deg);
}

.panel__custom-select-menu {
    position: fixed;
    z-index: 1200;
    box-sizing: border-box;
    padding: 6px;
    border-radius: 12px;
    border: 1px solid var(--panel-select-menu-border);
    background: var(--panel-select-menu-bg);
    box-shadow:
        0 12px 24px rgba(0, 0, 0, 0.24),
        inset 0 1px 0 rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
}

.panel__custom-select-option {
    width: 100%;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--panel-select-card-text);
    text-align: left;
    font: inherit;
    font-size: 12px;
    line-height: 1.3;
    padding: 7px 10px;
    cursor: pointer;
    transition: background-color 0.16s ease, color 0.16s ease;
}

.panel__custom-select-option--active {
    background: var(--panel-select-menu-hover-bg);
}

.panel__custom-select-option--selected {
    background: var(--panel-select-menu-selected-bg);
    color: var(--panel-select-card-text);
    box-shadow: inset 0 0 0 1px var(--panel-select-menu-selected-border);
}
</style>
