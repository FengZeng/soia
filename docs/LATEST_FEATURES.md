# Soia Player: Latest Features & Implementation Documentation

This document outlines the latest advanced features added to the Soia player, sorted by their implementation architecture and UI changes. 

## 1. Video Zoom with Hotkey Triggers
- **Description:** Allows manual zooming of the video surface to inspect details or manually crop out black bars.
- **Implementation Changes:** 
  - Bound `video-zoom` to the `onZoom` handler in `App.vue`.
  - Configured `usePlaybackShortcuts.ts` to capture `=` (Zoom In) and `-` (Zoom Out) keydown events to progressively increment or decrement the zoom scale linearly.
  - Passes the log2 converted scale to the `mpv_run_command` backend.
  - Triggers the OSD overlay (`showZoomOverlay`) to provide real-time visual feedback of the current zoom level.

## 2. Playback Speed - Precision Slider & Hotkeys (±0.1x)
- **Description:** Granular control over the video playback speed beyond standard presets.
- **Implementation Changes:**
  - Added a `ControlSlider` to the footer of the existing Speed Menu in `RightControls.vue`, configured to map values from `0.1x` to `4.0x` in steps of `0.1x`.
  - Configured `usePlaybackShortcuts.ts` to map the `[` key (decrease by 0.1x) and `]` key (increase by 0.1x) to seamlessly invoke `speed.setSpeed(rate)`.
  - Bound the slider's `@change` event to `set-speed` which synchronously updates the `mpv` speed property and displays the OSD feedback badge.

## 3. 3D Audio Virtual Surround with Hotkey Triggers
- **Description:** A highly compatible audio filter chain that mimics Virtual Surround sound natively without requiring external `libmysofa` libraries. Includes custom presets and fine-grained sliders.
- **Implementation Changes:**
  - Created a robust DSP approach using `extrastereo` (width), `aecho` (ambience), `treble`/`bass` (EQ), and `dynaudnorm` (dynamic boost) inside the `buildFilterChain` function in `useSurroundSound.ts`.
  - Integrated a new UI button in `RightControls.vue` to trigger a floating menu (`showSurroundMenu`) displaying presets (Movies, Music, Gaming) and individual DSP sliders.
  - Wired advanced keyboard chords in `usePlaybackShortcuts.ts`:
    - `Cmd+Shift+E`: Toggle 3D Audio effect entirely.
    - `Option+Shift+1/2/3`: Select specific presets.
    - `Option+Shift+[S/A/C/B/D]` + `+/-`: Increment/Decrement Surround Depth, Ambience, Clarity, Bass Boost, or Dynamic Boost.
  - Linked parameter changes to automatically enable the surround effect and trigger the OSD overlay via `usePlaybackOverlays.ts`.

## 4. Zoom Crop & Aspect Ratios
- **Description:** A dedicated interface panel to force-override the video's aspect ratio or manually zoom.
- **Implementation Changes:**
  - Added a new Crop icon button to the `RightControls.vue` toolbar, alongside a new floating menu (`showCropMenu`).
  - Added preset Aspect Ratio override buttons (`Auto`, `16:9`, `16:10`, `4:3`, `21:9`, `2.35:1`) which emit `set-aspect-ratio`.
  - Added a dedicated UI Zoom slider (0.5x to 3.0x scale) to the panel.
  - Wired `set-aspect-ratio` in `App.vue` to execute `invoke("mpv_run_command", { args: ["set", "video-aspect-override", ratio] })`.
  - Updated `useMenuControls.ts` to handle mutual exclusivity of the new `showCropMenu` state alongside existing menus.
