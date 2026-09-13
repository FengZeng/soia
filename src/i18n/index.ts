import {
    LANGUAGE_SETTING_LABEL,
    LANGUAGE_SIMPLIFIED_CHINESE_OPTION,
    type SettingItem,
} from "../mock/settings";
import { ref, readonly } from "vue";
import { locale as englishLocale } from "./locales/en";
import { locale as simplifiedChineseLocale } from "./locales/zh-CN";

// Add a locale ID and its dictionary below when another language is introduced.
export type SettingsLocale = typeof englishLocale | typeof simplifiedChineseLocale;

export const DEFAULT_SETTINGS_LOCALE: SettingsLocale = "en";

const activeSettingsLocale = ref<SettingsLocale>(DEFAULT_SETTINGS_LOCALE);

export const settingsLocale = readonly(activeSettingsLocale);

export const getSettingsLocale = (): SettingsLocale => activeSettingsLocale.value;

export const setSettingsLocale = (locale: SettingsLocale): void => {
    activeSettingsLocale.value = locale;
};

import { messages as chineseTranslations, translateDynamic } from "./locales/zh-CN";

export const normalizeSettingsLocale = (value: string | undefined): SettingsLocale =>
    value === LANGUAGE_SIMPLIFIED_CHINESE_OPTION
        ? "zh-CN"
        : DEFAULT_SETTINGS_LOCALE;

export const extractSettingsLocale = (
    groups?: readonly {
        items?: readonly { label?: string; value?: string }[];
    }[],
): SettingsLocale => {
    const value = groups
        ?.flatMap((group) => group.items ?? [])
        .find((item) => item.label === LANGUAGE_SETTING_LABEL)?.value;
    return normalizeSettingsLocale(value);
};

export const translateSettingsText = (
    locale: SettingsLocale,
    text: string | undefined | null,
): string => {
    if (!text) return "";
    if (locale !== "zh-CN") return text;
    return chineseTranslations[text] ?? translateDynamic(text) ?? text;
};

export const translateSettingItemLabel = (
    locale: SettingsLocale,
    item: Pick<SettingItem, "label" | "displayLabel">,
): string => translateSettingsText(locale, item.displayLabel ?? item.label);

export const translateSettingOption = (
    locale: SettingsLocale,
    option: string,
): string => translateSettingsText(locale, option);
