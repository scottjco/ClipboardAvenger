Clipboard Avenger

Run Clipboard Avenger.exe. It lives only in the system tray (check the ^ overflow arrow). Left-click its CA-and-sword icon to toggle ON/OFF: OFF darkens it, ON restores it. Right-click for settings:
  Turn Off / Turn On: pause or resume clipboard watching; shows the available action without a checkmark.
  --------------------
  Compression: PNG, JPG (default), WEBP, or Same as Source.
    --------------------
    JPEG quality: 50%, 55%, 60%, 65%, 70%, 75%, 80%, 85%, 90%, 95%, or 100%.
  Resize: Don't, 75%, 50%, or 25%.
  Save:
    Save Original: keep a copy before resizing or compression.
    Save Avenged: keep a copy of the smaller processed image.
    ...: choose another folder for saved copies; a selected custom path appears beside it in parentheses.
  --------------------
  Start with Windows: launch automatically when you sign in.
  Exit: quit.

All settings are saved immediately in %LOCALAPPDATA%\Clipboard Avenger\settings.ini and restored when the EXE next runs. With no INI file, the defaults are ON, Compression JPG, JPEG quality 95%, Resize Don't, Save Original unchecked, Save Avenged unchecked, and Start with Windows unchecked. Running the EXE twice does not start a second instance.

When upgrading from Desktop Image Avenger, the first renamed launch migrates the previous INI only if the new Clipboard Avenger configuration directory does not yet exist. Existing settings, including Start with Windows, are retained. Deleting the new INI after migration restores the defaults rather than importing the old settings again. Legacy image copies are left in place.

Start with Windows registers this EXE for your Windows account only and needs no administrator permission. Keep the EXE in its chosen location; if you move it, launch it once from its new location to update the startup path. Automatic launches restore every saved option, including ON/OFF. Exit quits the current session without disabling future automatic launches. The renamed app replaces its previous startup registration so Windows does not launch both names.

Every supported clipboard image is considered, including images that are already compressed. There is no minimum size threshold. Resize reduces both width and height to the selected percentage; Don't keeps the original dimensions. Compression selects PNG, JPG, WEBP, or Same as Source. JPG is the default and uses the selected JPEG quality. PNG and WEBP use lossless compression. Same as Source retains the detected format; raw Windows bitmaps use BMP. Lower quality percentages mean stronger compression and lower image quality; they are not a guaranteed percentage reduction in file size. Transparency is always preserved: JPG and alpha-unsafe source formats fall back to PNG. WEBP preserves 8-bit transparency; high-depth transparent inputs fall back to PNG to retain their alpha. With Resize Don't, PNG preserves all pixels and alpha values; resizing changes the pixel dimensions and values.

The clipboard is replaced only when the processed file is strictly smaller than the source. Save Avenged also saves only these smaller results. Text and non-image files are ignored. Animated files are skipped to preserve animation. Setting changes apply to future source-image scans; the app skips its own generated copies instead of repeatedly processing them.

Save Original and Save Avenged control extra archive copies. Both versions go directly into the same selected folder, with original- and avenged- filename prefixes. The default location is a Saved folder beside the EXE; the ... item selects another folder. Original encoded image data is saved untouched; a raw clipboard bitmap is saved as PNG. Source files are never overwritten.

If a write to the chosen custom save folder fails, the app immediately tries the default Saved folder for that copy. It counts one failure per image save batch, even if both archive copies fail. On the tenth failed attempt, it clears the custom folder setting and uses the default folder from then on. This count is saved across restarts and resets when another folder is chosen. Save failures, including failures in the fallback folder, are silent and do not interrupt clipboard processing.

Clipboard working copies are kept separately in your Pictures folder\Clipboard Avenger\Clipboard, regardless of the Save checkboxes. This location supports reliable Discord pasting and keeps file references usable for later pastes, including references retained in clipboard history. The app does not edit Windows+V history. Old working copies can be removed when no longer needed; deleting a referenced file can make an older paste unavailable. Settings and logs stay in AppData.

Images paste as file attachments, with filenames, explicit sizes, and indexed file contents supplied through the Windows OLE clipboard. Multiple copied image files are supported. Mixed file selections retain non-image files and any images that do not shrink. Windows' current bitmap clipboard holds one image. Scanned content is cached (256 recent items per run), and a busy guard permits only one scan at a time. Clipboard changes during compression cancel replacement of the old selection.

Building from source on Windows: install Rust with its MSVC toolchain and the Visual Studio C++ Build Tools, then open a terminal in this folder.
Build: cargo build --release --locked
Test: cargo test --locked
Built EXE: target\release\clipboard-avenger.exe
The distributed EXE is named Clipboard Avenger.exe.
No network connections or image uploads.
