# Virtual Surround Sound (3D Audio) Implementation

## Overview
This document outlines the architectural changes, features, and build instructions related to the 3D Audio Virtual Surround implementation within the Soia media player.

## Features
- **3D Audio Toggle**: Integrated directly into the player UI (`RightControls.vue`) to enable/disable virtual surround without interrupting playback.
- **DSP Presets**: Built-in configurations (Movies, Music, Gaming, Custom) mapping to specific DSP parameters.
- **Fine-Grained Sliders**: Granular control over Surround Depth, Ambience, Clarity, Bass Boost, and Dynamic Boost.
- **Global Keyboard Shortcuts**:
  - `Cmd+Shift+E`: Toggle 3D Audio state.
  - `Option+Shift+1/2/3`: Activate preset configurations.
  - `Option+Shift+[S/A/C/B/D]` + `+/-`: Incrementally adjust individual DSP parameters.
- **OSD Integration**: Real-time visual feedback via On-Screen Display badges when modifying DSP values through keyboard shortcuts.

## Architecture

The 3D Audio feature leverages `mpv`'s audio filter (`af`) subsystem, bypassing the need for external HRTF/SOFA files (`libmysofa`), which ensures strict compatibility across standard macOS `mpv` binaries. 

### DSP Filter Chain (`useSurroundSound.ts`)
The `buildFilterChain` function translates the reactive `SurroundState` into a comma-separated `af` command string sent to `mpv` via IPC:
- **Stereo Widening**: `extrastereo` (Surround Depth)
- **Reverb / Ambience**: `aecho`
- **EQ**: `bass` and `treble` (Clarity)
- **Normalization**: `dynaudnorm` (Dynamic Boost)

State persistence is handled via `saveUiState`, and the filter chain is debounced before application to prevent IPC flooding.

### UI Integration (`RightControls.vue` & `PlayerControls.vue`)
- The UI triggers `setParam`, `setEnabled`, and `setPreset` within the `useSurroundSound` composable.
- Menu visibility state (`showSurroundMenu`) is propagated up to `App.vue` via `toggle-menu` events, adhering to the mutual exclusivity pattern of other floating player menus.

### Keybindings & OSD (`usePlaybackShortcuts.ts` & `usePlaybackOverlays.ts`)
- Key events are captured and tracked globally. Chords (e.g., holding `Option+Shift+S` and tapping `+`) are processed by tracking a `Set` of active keys.
- Parameter changes invoke the `showSurroundOverlay` callback, which updates the reactive `surroundOverlayText` rendered by `PlaybackOverlays.vue`.

## macOS Build Instructions

The build process relies on a custom environment script to supply necessary dependencies (such as the `mpv` library).

1. Source the dependencies environment variables:
   ```bash
   source ../Dependencies/env.sh
   ```
2. Install Node dependencies (if not already installed):
   ```bash
   pnpm install
   ```
3. Run the development server:
   ```bash
   pnpm tauri dev
   ```
4. Build the release binary:
   ```bash
   pnpm build
   ```
   *(Ensure `pnpm` is available in your PATH after sourcing the environment script).*
