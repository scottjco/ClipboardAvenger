Clipboard Avenger

Version 1.0.3 - Windows - Source only on GitHub.
The GitHub repository provides source code, build files, and icon assets. No prebuilt EXE is provided; build the app yourself using the instructions at the end of this file.

Run Clipboard Avenger.exe. It lives only in the system tray (check the ^ overflow arrow). Left-click its CA-and-sword icon to toggle ON/OFF: OFF darkens it, ON restores it. Right-click for settings:
  Turn Off / Turn On: pause or resume clipboard watching; shows the available action without a checkmark.
  --------------------
  Compression: PNG, JPG (default), or Same as Source.
    --------------------
    JPEG quality: 50%, 55%, 60%, 65%, 70%, 75%, 80%, 85%, 90%, 95%, or 100%.
  Resize: Don't, 75%, 50%, or 25%.
  Misc:
    Automatic WebP to PNG conversion: checked by default; convert WebP for easier pasting into painting apps.
  Save:
    Save Original: keep a copy before resizing or compression.
    Save Avenged: keep a copy of the compressed or converted result.
    ...: choose another folder for saved copies; a selected custom path appears beside it in parentheses.
  --------------------
  Start with Windows: launch automatically when you sign in.
  Exit: quit.

All settings are saved immediately in %LOCALAPPDATA%\Clipboard Avenger\settings.ini and restored when the EXE next runs. With no INI file, the defaults are ON, Compression JPG, JPEG quality 95%, Automatic WebP to PNG conversion checked, Resize Don't, Save Original unchecked, Save Avenged unchecked, and Start with Windows unchecked. The new option is saved as AutomaticWebpToPng and is also enabled when an older INI does not contain it. A saved Compression=WEBP setting now loads as JPG. Running the EXE twice does not start a second instance.

When upgrading from Desktop Image Avenger, the first renamed launch migrates the previous INI only if the new Clipboard Avenger configuration directory does not yet exist. Existing settings, including Start with Windows, are retained. Deleting the new INI after migration restores the defaults rather than importing the old settings again. Legacy image copies are left in place.

Start with Windows registers this EXE for your Windows account only and needs no administrator permission. Keep the EXE in its chosen location; if you move it, launch it once from its new location to update the startup path. Automatic launches restore every saved option, including ON/OFF. Exit quits the current session without disabling future automatic launches. The renamed app replaces its previous startup registration so Windows does not launch both names.

Every supported clipboard image is considered, including images that are already compressed. There is no minimum size threshold. Resize reduces both width and height to the selected percentage; Don't keeps the original dimensions. Compression selects PNG, JPG, or Same as Source. JPG is the default and uses the selected JPEG quality. PNG uses lossless compression. Lower quality percentages mean stronger compression and lower image quality; they are not a guaranteed percentage reduction in file size. Transparency is always preserved: JPG and alpha-unsafe source formats fall back to PNG. With Resize Don't, PNG preserves decoded pixels and alpha values; resizing changes the pixel dimensions and values.

Misc appears after Resize and before Save. Its Automatic WebP to PNG conversion checkbox is on by default. When enabled, WebP becomes PNG even if the PNG is larger. If JPG is selected and all source pixels are fully opaque, a smaller JPEG is used instead; if JPEG does not shrink the image, WebP still becomes PNG. Transparent WebP always becomes PNG. Opaque PNG can also become JPEG when JPG is selected, but only if JPEG is smaller. The app checks actual alpha values before resizing; an alpha channel whose pixels are all opaque does not prevent JPEG conversion.

Outside automatic WebP conversion, the clipboard is replaced only when the processed file is strictly smaller than the source. With the checkbox off, Same as Source retains the detected format, including WebP; raw Windows bitmaps use BMP. Save Avenged saves results chosen for replacement, including WebP conversions that are larger than the original. Text and non-image files are ignored. Animated files are skipped to preserve animation. Setting changes apply to future source-image scans; the app skips its own generated copies instead of repeatedly processing them, except for a one-time WebP conversion that preserves the already-processed dimensions.

Save Original and Save Avenged control extra archive copies. Both versions go directly into the same selected folder, with original- and avenged- filename prefixes. The default location is a Saved folder beside the EXE; the ... item selects another folder. Original encoded image data is saved untouched; a raw clipboard bitmap is saved as PNG. Source files are never overwritten.

If a write to the chosen custom save folder fails, the app immediately tries the default Saved folder for that copy. It counts one failure per image save batch, even if both archive copies fail. On the tenth failed attempt, it clears the custom folder setting and uses the default folder from then on. This count is saved across restarts and resets when another folder is chosen. Save failures, including failures in the fallback folder, are silent and do not interrupt clipboard processing.

Clipboard working copies are kept separately in your Pictures folder\Clipboard Avenger\Clipboard, regardless of the Save checkboxes. This location supports reliable Discord pasting and keeps file references usable for later pastes, including references retained in clipboard history. The app does not edit Windows+V history. Old working copies can be removed when no longer needed; deleting a referenced file can make an older paste unavailable. Settings and logs stay in AppData.

Images paste as file attachments, with filenames, explicit sizes, and indexed file contents supplied through the Windows OLE clipboard. Single images also include standard Windows bitmap formats for painting apps, representing the processed image with its resized dimensions and transparency. Multiple copied image files are supported. Mixed file selections retain non-image files and images that are neither compressed nor converted. Windows' current bitmap clipboard holds one image. Scanned content is cached (256 recent items per run), and a busy guard permits only one scan at a time. Clipboard changes during compression cancel replacement of the old selection.

Building from source on Windows: install Rust with its MSVC toolchain and the Visual Studio C++ Build Tools, then open a terminal in this folder.
Build: cargo build --release --locked
Test: cargo test --locked
Built EXE: target\release\clipboard-avenger.exe
You can rename your built EXE to Clipboard Avenger.exe and place it in your chosen folder.
No network connections or image uploads.

Changelog

1.0.3 - Current release
- Removed WEBP from Compression. Older saved WEBP selections now load as JPG.
- Added Misc between Compression and Resize, with Automatic WebP to PNG conversion checked by default and remembered in the INI.
- WebP conversion produces PNG even when it is larger, preserving transparency. With JPG selected, fully opaque WebP uses a smaller JPEG when possible and falls back to PNG otherwise.
- Checks source alpha values before resizing. Opaque PNG is eligible for smaller JPEG output with JPG selected; transparent pixels retain an alpha-safe format.
- Retained native bitmap clipboard support for painting apps. Save Avenged includes converted results.

1.0.2 - Changes since the initial source release
- Left-click the tray icon to toggle ON/OFF; OFF darkens the icon and startup restores the matching saved-state icon.
- Added Compression above Resize: PNG, JPG, WEBP, or Same as Source. JPG is the default and transparency is preserved with an alpha-safe output format.
- Moved JPEG quality into Compression, below a separator. PNG and WEBP use lossless compression independently of JPEG quality.
- The top tray action now reads Turn Off or Turn On, without a checkmark.
- Single processed images now include native Windows bitmap clipboard formats alongside their file attachment, allowing bitmap-only painting-app paste operations. Existing processed files can receive the missing formats too.
- Compressed attachment bytes, filenames, and sizes remain intact. Compression choice is saved in the INI; EXE metadata now identifies version 1.0.2.
- Updated source-only build instructions and added AI disclosure and release-numbering records.

1.0.0 - Initial source release
- Rust Windows tray app with CA-and-sword icon, ON/OFF control, JPEG compression for opaque images, and PNG for transparency.
- Replaced only smaller results, ignored non-images, and supported multiple image files through named, sized, indexed attachments for apps such as Discord.
- JPEG quality 50%-100% in 5% steps; Resize Don't, 75%, 50%, or 25%.
- Save Original and Save Avenged in one folder, custom-folder selection, silent fallback to Saved, and clearing of a custom path after ten failed attempts.
- INI persistence and optional Start with Windows; defaults ON, JPEG 95%, no resize, saves OFF, startup OFF.
- Single-scan busy guard, recent-image cache, clipboard-change protection, and single-instance operation.

AI disclosure

This project was made entirely with AI, if you have a problem with that. You do you, I'm not remotely pretending I made this. ChatGPT did it entirely by itself. I just directed it.
