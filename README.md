# Clipboard Avenger

<img src="clipboard-avenger.png" alt="Clipboard Avenger CA and sword icon" width="160">

A Windows system tray app written in Rust that makes clipboard images smaller before you paste them. It works locally, with no network connections or image uploads.

Opaque images become JPEG at the selected quality. Images with any transparency use lossless PNG compression. The app replaces an image only when the result is strictly smaller, and never overwrites source files. Images paste as file attachments, including in Discord.

## Features and defaults

Right-click the tray icon to change settings. Settings save immediately and restore on the next launch. These defaults apply when no INI file is present:

| Setting | Options | Default |
| --- | --- | --- |
| Clipboard watching | ON / OFF | ON |
| Resize | Don't, 75%, 50%, 25% | Don't |
| JPEG quality | 50%–100%, in 5% steps | 95% |
| Save Original | Keep an extra copy of the source image | OFF |
| Save Avenged | Keep an extra copy of a smaller processed image | OFF |
| Start with Windows | Launch when you sign in | OFF |

Lower JPEG quality means stronger compression; the percentage is not a promised reduction in file size. Resize scales both dimensions. Transparent PNG output preserves pixels and alpha when Resize is set to Don't; resizing changes the dimensions and pixel values.

- Text and non-image clipboard content are ignored. Mixed file selections retain non-image files and images that do not shrink.
- Multiple copied image files are supported. Animated files are skipped to preserve animation.
- A single scan runs at a time, recent scanned content is cached, and clipboard changes during processing cancel replacement of the old selection.
- Only one app instance runs at a time. Exit quits the current session without changing the saved startup preference.

## Build and run

Use Windows with Rust's MSVC toolchain and the Visual Studio C++ Build Tools installed. In the repository directory, run:

```powershell
cargo test --locked
cargo build --release --locked
```

Run `target\release\clipboard-avenger.exe`. It appears in the system tray, possibly under the overflow arrow. The icon is embedded during the build. This repository contains source and icon assets, with no prebuilt executable.

## Settings and saved images

| Data | Location |
| --- | --- |
| Settings | `%LOCALAPPDATA%\Clipboard Avenger\settings.ini` |
| Activity log | `%LOCALAPPDATA%\Clipboard Avenger\activity.log` |
| Clipboard working copies | Your Windows Pictures folder, under `Clipboard Avenger\Clipboard` |
| Optional saved copies | `Saved` beside the executable, or the folder selected through **Save → ...** |

Original and avenged archive copies share the same folder, with `original-` and `avenged-` filename prefixes. Encoded originals are copied untouched; raw clipboard bitmaps are saved as PNG. Save Avenged archives only results that are smaller than the source.

If the chosen custom folder cannot be written to, the app silently falls back to `Saved` beside the executable for that copy. It counts one failure per image save batch. After ten failed attempts, it clears the custom folder setting. The failure count persists across restarts and resets when another folder is chosen. Save failures do not interrupt clipboard processing.

Clipboard working copies are separate from optional saved copies and are retained so later pastes and clipboard history can still refer to them. Removing a working copy can make an older paste unavailable.

Start with Windows applies only to your Windows account and needs no administrator permission. If you move the executable, run it once from its new location to update the startup path. Existing settings can be migrated from the app's former name, Desktop Image Avenger; see the [full usage notes](README.txt) for details.
