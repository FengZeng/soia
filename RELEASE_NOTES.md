# Release Notes

## Important (macOS)

macOS may show "Soia is damaged and can't be opened" or say it cannot verify the app is free of malware.
This happens because the app is not yet signed with an Apple Developer ID certificate, so macOS may block it on first launch.

Workaround (Recommended):
1. Right-click Soia.app
2. Click "Open"
3. Click "Open" again in the dialog

If that doesn't work, run:
```bash
sudo xattr -r -d com.apple.quarantine /Applications/Soia.app
```

You can also go to System Settings > Privacy & Security and click "Open Anyway" (it appears after a blocked launch attempt).

The app is open-source and its code is publicly available for anyone to inspect.

## [0.2.10] - 2026-08-08

### Highlights

* **Remote Controller: Playlists and Network Media**
  The browser-based Remote Controller can now browse and play playlists, as well as browse and play media from configured network sources. Network browsing uses the same remembered folder as the desktop app, so you can pick up where you left off on another device.

### Fixes

* Fixed a Windows issue where switching videos or leaving playback could leave the app window fully transparent.
