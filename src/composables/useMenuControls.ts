import { ref, type Ref } from "vue";

type MenuRefs = {
    showAudioMenu: Ref<boolean>;
    showSubMenu: Ref<boolean>;
    showSubtitleAdvancedSettings: Ref<boolean>;
};

type SpeedRefs = {
    showSpeedMenu: Ref<boolean>;
};

type SettingsRefs = {
    showSettingsMenu: Ref<boolean>;
};

export const useMenuControls = (
    tracks: MenuRefs,
    speed: SpeedRefs,
    settings: SettingsRefs,
) => {
    const showSurroundMenu = ref(false);

    const hideAllMenus = () => {
        tracks.showAudioMenu.value = false;
        tracks.showSubMenu.value = false;
        tracks.showSubtitleAdvancedSettings.value = false;
        speed.showSpeedMenu.value = false;
        settings.showSettingsMenu.value = false;
        showSurroundMenu.value = false;
    };

    const toggleMenu = (menuName: "audio" | "sub" | "speed" | "settings" | "surround") => {
        const wasOpen = {
            audio: tracks.showAudioMenu.value,
            sub: tracks.showSubMenu.value,
            speed: speed.showSpeedMenu.value,
            settings: settings.showSettingsMenu.value,
            surround: showSurroundMenu.value,
        };

        hideAllMenus();

        if (menuName === "audio" && !wasOpen.audio) {
            tracks.showAudioMenu.value = true;
        }
        if (menuName === "sub" && !wasOpen.sub) {
            tracks.showSubMenu.value = true;
        }
        if (menuName === "speed" && !wasOpen.speed) {
            speed.showSpeedMenu.value = true;
        }
        if (menuName === "settings" && !wasOpen.settings) {
            settings.showSettingsMenu.value = true;
        }
        if (menuName === "surround" && !wasOpen.surround) {
            showSurroundMenu.value = true;
        }
    };

    const closeAllMenus = (event: MouseEvent) => {
        const target = event.target as HTMLElement | null;
        if (!target?.closest(".track-menu-container")) {
            hideAllMenus();
        }
    };

    return {
        showSurroundMenu,
        hideAllMenus,
        toggleMenu,
        closeAllMenus,
    };
};
