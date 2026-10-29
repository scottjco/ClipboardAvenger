# Clipboard Avenger

**Version 1.0.3 · Windows · Source only**

<img src="clipboard-avenger.png" alt="Clipboard Avenger CA and sword icon" width="160">

A Windows system tray app written in Rust that makes clipboard images smaller before you paste them. It works locally, with no network connections or image uploads.

This repository contains the Rust source, build files, and icon assets. **No prebuilt EXE is provided. You must build the app yourself on Windows** using the instructions below.

Choose PNG, JPG, or Same as Source under Compression. JPG is the default. Normal compression replaces an image only when the result is strictly smaller. Automatic WebP conversion can replace WebP with PNG even when PNG is larger, to make the image easier to paste into painting apps. Source files are never overwritten. Images paste as file attachments, including in Discord.

Version 1.0.3 adds automatic WebP conversion under Misc and removes WebP from the compression choices. Native bitmap clipboard support remains available for painting apps alongside file attachments, with transparency preserved.

## Features and defaults

Left-click the tray icon to turn watching ON or OFF. OFF uses a darker version of the icon; ON restores the original artwork. Right-click to change settings. Settings save immediately and restore on the next launch, including the matching icon. These defaults apply when no INI file is present:

| Setting | Options | Default |
| --- | --- | --- |
| Clipboard watching | Turn Off / Turn On (action, no checkmark) | ON |
| Compression | PNG, JPG, Same as Source | JPG |
| Misc → Automatic WebP to PNG conversion | Convert WebP for easier pasting into painting apps | ON |
| Resize | Don't, 75%, 50%, 25% | Don't |
| Compression → JPEG quality | 50%–100%, in 5% steps | 95% |
| Save Original | Keep an extra copy of the source image | OFF |
| Save Avenged | Keep an extra copy of a compressed or converted result | OFF |
| Start with Windows | Launch when you sign in | OFF |

The menu order is Compression, Misc, then Resize. Compression's three format choices are followed by a separator and the JPEG quality submenu. Lower JPEG quality means stronger JPG compression; the percentage is not a promised reduction in file size. PNG uses lossless compression, independent of JPEG quality. Resize scales both dimensions; Don't preserves the original dimensions.

**Misc → Automatic WebP to PNG conversion** is checked by default and saved as `AutomaticWebpToPng` in the INI. When enabled, WebP becomes PNG even if conversion increases the file size. If JPG is selected and every source pixel is fully opaque, the app first tries JPEG at the selected quality: it uses JPEG if smaller, otherwise PNG. Transparent WebP always becomes PNG. Opaque PNG is also eligible for JPEG when JPG is selected, but only if the JPEG is smaller. An alpha channel alone does not make an image transparent; the app checks the pixels before resizing.

With automatic WebP conversion off, all replacements must be smaller. Same as Source retains the detected input format, including WebP (raw Windows bitmaps use BMP). Transparency is always preserved: JPG and alpha-unsafe source formats fall back to PNG. With Resize set to Don't, PNG preserves the decoded pixels and alpha values. An older saved `Compression=WEBP` setting now loads as JPG.

- Text and non-image clipboard content are ignored. Mixed file selections retain non-image files and images that are neither compressed nor converted.
- Multiple copied image files are supported. Animated files are skipped to preserve animation.
- A single image also offers standard Windows bitmap formats for painting apps, alongside the compressed file attachment. The bitmap represents the processed image, including its resized dimensions and transparency.
- A single scan runs at a time, recent scanned content is cached, and clipboard changes during processing cancel replacement of the old selection.
- Only one app instance runs at a time. Exit quits the current session without changing the saved startup preference.

## Build and run

Prerequisites: Windows, Rust's MSVC toolchain, and the Visual Studio C++ Build Tools with the Windows SDK installed.

Clone this repository, or select **Code → Download ZIP** and extract the source. Open a terminal in the folder containing `Cargo.toml`, then run:

```powershell
cargo test --locked
cargo build --release --locked
```

Run the EXE you built at `target\release\clipboard-avenger.exe`. It appears in the system tray, possibly under the overflow arrow. The icon is embedded during the build. You can copy your built EXE to another folder before enabling Start with Windows.

## Settings and saved images

| Data | Location |
| --- | --- |
| Settings | `%LOCALAPPDATA%\Clipboard Avenger\settings.ini` |
| Activity log | `%LOCALAPPDATA%\Clipboard Avenger\activity.log` |
| Clipboard working copies | Your Windows Pictures folder, under `Clipboard Avenger\Clipboard` |
| Optional saved copies | `Saved` beside the executable, or the folder selected through **Save → ...** |

Original and avenged archive copies share the same folder, with `original-` and `avenged-` filename prefixes. Encoded originals are copied untouched; raw clipboard bitmaps are saved as PNG. Save Avenged archives results selected for clipboard replacement, including automatic WebP conversions that are larger than the source.

If the chosen custom folder cannot be written to, the app silently falls back to `Saved` beside the executable for that copy. It counts one failure per image save batch. After ten failed attempts, it clears the custom folder setting. The failure count persists across restarts and resets when another folder is chosen. Save failures do not interrupt clipboard processing.

Clipboard working copies are separate from optional saved copies and are retained so later pastes and clipboard history can still refer to them. Removing a working copy can make an older paste unavailable.

Start with Windows applies only to your Windows account and needs no administrator permission. If you move the executable, run it once from its new location to update the startup path. Existing settings can be migrated from the app's former name, Desktop Image Avenger; see the [full usage notes](README.txt) for details.

## Changelog

### 1.0.3 — Current release

- Removed WEBP from the Compression choices. Existing saved WEBP selections now load as JPG.
- Added **Misc** between Compression and Resize, with a checked-by-default **Automatic WebP to PNG conversion** option that is remembered in the INI.
- Automatic conversion replaces WebP with PNG even when PNG is larger, preserving transparency for painting-app compatibility. If JPG is selected, fully opaque WebP uses JPEG instead when it is smaller; otherwise it becomes PNG.
- Checks actual source alpha values before resizing. Fully opaque PNG can become a smaller JPEG when JPG is selected; any transparent pixels keep an alpha-safe format.
- Kept native bitmap clipboard support for painting apps, and included converted results in Save Avenged.

### 1.0.2

Changes since the initial source release:

- Added left-click toggling on the tray icon. OFF uses a darker icon; ON restores the original icon. The icon also reflects the saved state at startup.
- Added **Compression** above Resize, with PNG, JPG, WEBP, and Same as Source. JPG is the default; transparent images retain an alpha-safe output format.
- Moved **JPEG quality** into Compression, below the format choices and a separator. PNG and WEBP use lossless compression independently of JPEG quality.
- Changed the top tray item to **Turn Off** while enabled and **Turn On** while disabled, with no checkmark.
- Added native Windows bitmap clipboard formats alongside single-image file attachments for painting apps. Previously, processed JPEGs were offered only as attachments, which prevented bitmap-only paste operations.
- Kept the compressed attachment's bytes, filename, and size intact while adding bitmap compatibility. Existing processed clipboard files can also receive the missing bitmap formats.
- Saved the compression choice in the INI and set the EXE's version metadata to 1.0.2.
- Updated the source-only build instructions and added the AI disclosure and release-numbering records.

### 1.0.0 — Initial source release

- Introduced the Windows tray app in Rust, with the CA-and-sword icon and an ON/OFF control.
- Compressed opaque clipboard images to JPEG, preserved transparency through PNG, and replaced images only when the output was smaller. Non-image clipboard content was ignored.
- Added JPEG quality choices from 50% to 100% in 5% steps, plus Don't / 75% / 50% / 25% resizing.
- Supported multiple copied image files and supplied named, sized, indexed attachments for file-aware apps such as Discord.
- Added optional Save Original and Save Avenged copies in the same folder, custom-folder selection, and silent fallback to Saved. Ten failed custom-folder attempts clear the selection.
- Remembered settings in an INI and added optional Start with Windows. Fresh defaults were ON, JPEG 95%, no resizing, both save options OFF, and startup OFF.
- Added a single-scan busy guard, recent-image cache, protection against replacing a newer clipboard selection, and single-instance operation.

## AI disclosure

This project was made entirely with AI, if you have a problem with that. You do you, I'm not remotely pretending I made this. ChatGPT did it entirely by itself. I just directed it.
