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

## [0.2.13] - 2026-10-18

### Highlights

* **Add network sources from the Remote Controller**
  Create WebDAV, SMB, and HTTP DLNA connections directly from the browser-based Remote Controller, without returning to the desktop app.

* **Safer local HTTPS WebDAV connections**
  Soia can now trust and pin private or self-signed certificates from local HTTPS WebDAV servers. A saved connection continues to require the same certificate, protecting it from unexpected certificate changes.

* **Easier network address entry**
  WebDAV and HTTP DLNA addresses no longer require an explicit URL scheme. Addresses such as `nas.local:5244/dav` are automatically treated as HTTP URLs.

* **Playback and settings cleanup** ([#33](https://github.com/FengZeng/soia/issues/33))
  Fixed automatic audio track selection after playback stops, and removed the community link from Settings > About.
