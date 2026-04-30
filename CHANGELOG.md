# Changelog

## [0.1.8] - 2026-04-30

### Features

- Added a Disable Subtitles option to force subtitles off on file load.
- Added persistent manual window state restoration.
- Added a link to the Soia subreddit in Settings.

### Improvements

- Added current playback highlighting and folder path context in the Network panel.
- Added a global video settings toggle with persisted profile support and per-file local adjustments.
- Default compact mode remains enabled to reduce accidental window decorations.
- Improved sibling Auto-Play Next behavior for local media and WebDAV media.

## [0.1.7] - 2026-04-24

### Features

- Added M3U playlist parsing support.

### Improvements

- Expanded macOS compatibility and now support macOS 13+.
- Stabilized WebDAV playback authentication and URL handling.

## [0.1.6] - 2026-04-18

### Features

- Added DLNA support for device discovery, media browsing, and stream playback.
- Added Playback Title display modes, including the option to hide the top title bar for a cleaner playback UI.

### Improvements

- Improved network browser header with Home navigation and better DLNA browsing paths.
- Redesigned the Home panel UI.
- Modularized protocol stack and unified browse/load command flow.

### Fixes

- Fixed an issue where the screen could sleep during active video playback.
- Improved robustness of the update check process and fixed update flow issues.

## [0.1.5] - 2026-04-11

### Features

- Added experimental support for Windows Portable and Linux AppImage builds.
- Added Anime-specialized mode with automatic detection and automatic shader application.
- Added per-playback shader toggle and active shader display.
- Added Factory Reset to quickly clear local data and restore default state.

### Improvements

- Improved Custom Shader settings with mode-aware behavior and cleaner controls.
- On Windows, Update now uses different behavior for Portable vs Setup installs.

## [0.1.4] - 2026-04-05

### Features

- Added dual subtitles support.
- Added custom shader support.
- Added experimental Wallpaper mode on Windows.
- Added image playback support for common formats, including playlist playback and Wallpaper mode combinations.

### Fixes

- Fixed a potential crash when quitting the app during playback.

## [0.1.2] - 2026-03-30

### Fixes

- Fixed an issue where the video window could appear transparent or fail to render on certain macOS versions (including macOS Tahoe).
- Added MoltenVK Vulkan ICD manifest setup for both development and packaged app runtime to ensure stable video output.

## [0.1.1] - 2026-03-28

First official release.

### Features

- MPV-backed local media playback.
- Picture in Picture (PiP) support on macOS and Windows.
- Borderless window mode on macOS, Windows, and Linux (Ubuntu Wayland).
- HDR and Dolby Vision playback on compatible media/hardware (not supported on Linux).
- Playback history with resume position and pin-to-top support.
- Multiple playlist support with sort modes and loop/shuffle behaviors.
- WebDAV media browsing and streaming.
- Buffering progress bar for network video playback.
- Download speed display when network video buffering is paused.
- Playback preferences (seek step, default speed, auto-play, skip intro).
- macOS platform integrations for media keys, Now Playing metadata, and artwork capture.

### Platform Notes

- Linux builds currently target Ubuntu Wayland sessions only (`X11` is not supported).
