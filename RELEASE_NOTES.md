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
xattr -r -d com.apple.quarantine /Applications/Soia.app
```

You can also go to System Settings > Privacy & Security and click "Open Anyway" (it appears after a blocked launch attempt).

The app is open-source and its code is publicly available for anyone to inspect.

## [0.2.12] - 2026-09-19

### Highlights

* **User mpv config file support** ([#12](https://github.com/FengZeng/soia/issues/12))
  Point Soia at your own mpv config file in Settings > General and it is loaded at startup.
  **Note:** not every mpv option is fully compatible with Soia's rendering and playback pipeline, so some settings may be ignored or behave differently. Changes take effect after a restart.

* **Correct HDR and Dolby Vision colors on macOS**
  Fixed washed-out or overly dark HDR and Dolby Vision playback on macOS with display-P3/PQ output hints and perceptual gamut mapping.

* **Sorting in the Network browser** ([#30](https://github.com/FengZeng/soia/issues/30))
  Sort network folders and files by name or date added, ascending or descending. Folders stay at the top in every sort mode.

* **Simplified Chinese support**
  Added a Language setting in Settings > General with Simplified Chinese (简体中文) alongside English.
