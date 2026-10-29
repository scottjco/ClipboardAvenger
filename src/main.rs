#![windows_subsystem = "windows"]
mod imaging;
mod ole_clipboard;
mod startup;
mod storage;
#[cfg(test)]
use image::DynamicImage;
use imaging::{dib_image, Compression};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering::*},
        Mutex, OnceLock,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        Com::CoTaskMemFree, DataExchange::*, LibraryLoader::*, Memory::*, Ole::*, Threading::*,
    },
    UI::{Shell::*, WindowsAndMessaging::*},
};
static ENABLED: AtomicBool = AtomicBool::new(true);
static QUALITY: AtomicU32 = AtomicU32::new(95);
static COMPRESSION: AtomicU32 = AtomicU32::new(Compression::Jpg as u32);
static AUTOMATIC_WEBP_TO_PNG: AtomicBool = AtomicBool::new(true);
static START_WITH_WINDOWS: AtomicBool = AtomicBool::new(false);
static RESIZE: AtomicU32 = AtomicU32::new(100);
static SAVE_ORIGINAL: AtomicBool = AtomicBool::new(false);
static SAVE_AVENGED: AtomicBool = AtomicBool::new(false);
static SAVE_FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);
static FOLDER_FAILURES: AtomicU32 = AtomicU32::new(0);
static REVISION: AtomicU32 = AtomicU32::new(0);
static BUSY: AtomicBool = AtomicBool::new(false);
static LAST: AtomicU32 = AtomicU32::new(0);
static PENDING: Mutex<Option<(u32, u32, ole_clipboard::Prepared, u32)>> = Mutex::new(None);
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
static SETTINGS_WRITE: Mutex<()> = Mutex::new(());
const QUALITY_MENU_BASE: u32 = 1000;
const RESIZE_MENU_BASE: u32 = 2000;
const COMPRESSION_MENU_BASE: u32 = 3000;
const AUTOMATIC_WEBP_MENU: u32 = 7;
#[derive(Debug, PartialEq)]
struct Settings {
    enabled: bool,
    jpeg_quality: u8,
    compression: Compression,
    automatic_webp_to_png: bool,
    start_with_windows: bool,
    resize_percent: u8,
    save_original: bool,
    save_avenged: bool,
    save_folder: String,
    folder_failures: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            jpeg_quality: 95,
            compression: Compression::Jpg,
            automatic_webp_to_png: true,
            start_with_windows: false,
            resize_percent: 100,
            save_original: false,
            save_avenged: false,
            save_folder: String::new(),
            folder_failures: 0,
        }
    }
}
impl Settings {
    fn from_ini(text: &str) -> Self {
        let mut settings = Self::default();
        let mut in_settings = false;
        for line in text.lines() {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.starts_with(';') || line.starts_with('#') || line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                in_settings = line.eq_ignore_ascii_case("[Settings]");
                continue;
            }
            if !in_settings {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("SaveFolder") {
                settings.save_folder = value.trim().to_owned();
                continue;
            }
            let value = value.split([';', '#']).next().unwrap_or("").trim();
            if key.trim().eq_ignore_ascii_case("Enabled") {
                match value.to_ascii_lowercase().as_str() {
                    "true" | "1" => settings.enabled = true,
                    "false" | "0" => settings.enabled = false,
                    _ => {}
                }
            } else if key.trim().eq_ignore_ascii_case("StartWithWindows") {
                match value.to_ascii_lowercase().as_str() {
                    "true" | "1" => settings.start_with_windows = true,
                    "false" | "0" => settings.start_with_windows = false,
                    _ => {}
                }
            } else if key.trim().eq_ignore_ascii_case("SaveOriginal") {
                if let Some(value) = parse_bool(value) {
                    settings.save_original = value;
                }
            } else if key.trim().eq_ignore_ascii_case("AutomaticWebpToPng") {
                if let Some(value) = parse_bool(value) {
                    settings.automatic_webp_to_png = value;
                }
            } else if key.trim().eq_ignore_ascii_case("SaveAvenged") {
                if let Some(value) = parse_bool(value) {
                    settings.save_avenged = value;
                }
            } else if key.trim().eq_ignore_ascii_case("ResizePercent") {
                if let Ok(value) = value.parse::<u8>() {
                    if [100, 75, 50, 25].contains(&value) {
                        settings.resize_percent = value;
                    }
                }
            } else if key.trim().eq_ignore_ascii_case("SaveFolderFailures") {
                settings.folder_failures = value.parse().unwrap_or(0);
            } else if key.trim().eq_ignore_ascii_case("Compression") {
                if let Some(compression) = Compression::parse(value) {
                    settings.compression = compression;
                }
            } else if key.trim().eq_ignore_ascii_case("JpegQuality") {
                if let Ok(quality) = value.parse::<u8>() {
                    if valid_quality(quality) {
                        settings.jpeg_quality = quality;
                    }
                }
            }
        }
        if settings.folder_failures >= 10 {
            settings.save_folder.clear();
            settings.folder_failures = 0;
        }
        settings
    }
    fn to_ini(&self) -> String {
        format!("; Clipboard Avenger settings\n; JPEG quality applies to JPG. PNG uses lossless compression.\n; AutomaticWebpToPng converts WebP even if PNG is larger. Smaller opaque JPG takes priority.\n; ResizePercent=100 means Don't. Empty SaveFolder uses Saved beside the EXE.\n[Settings]\nEnabled={}\nCompression={}\nAutomaticWebpToPng={}\nJpegQuality={}\nResizePercent={}\nSaveOriginal={}\nSaveAvenged={}\nSaveFolder={}\nSaveFolderFailures={}\nStartWithWindows={}\n", self.enabled, self.compression.label(), self.automatic_webp_to_png, self.jpeg_quality, self.resize_percent, self.save_original, self.save_avenged, self.save_folder, self.folder_failures, self.start_with_windows)
    }
}
fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}
fn valid_quality(quality: u8) -> bool {
    (50..=100).contains(&quality) && quality % 5 == 0
}
struct Cache {
    map: HashMap<String, Option<PathBuf>>,
    order: VecDeque<String>,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Clipboard Avenger")
}
fn legacy_home() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Desktop Image Avenger")
}
/// Discord refuses pasted files in AppData even when their bytes are valid.
/// Use the user's Pictures known folder (including redirected/OneDrive folders).
fn image_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| unsafe {
        let mut ptr = std::ptr::null_mut();
        let pictures =
            if SHGetKnownFolderPath(&FOLDERID_Pictures, 0, std::ptr::null_mut(), &mut ptr) >= 0
                && !ptr.is_null()
            {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let path = PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )));
                CoTaskMemFree(ptr as *const _);
                path
            } else {
                PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join("Pictures")
            };
        pictures.join("Clipboard Avenger")
    })
}
fn save() {
    let _write = SETTINGS_WRITE.lock().unwrap();
    let settings = Settings {
        enabled: ENABLED.load(Relaxed),
        jpeg_quality: QUALITY.load(Relaxed) as u8,
        compression: Compression::from_id(COMPRESSION.load(Relaxed)),
        automatic_webp_to_png: AUTOMATIC_WEBP_TO_PNG.load(Relaxed),
        start_with_windows: START_WITH_WINDOWS.load(Relaxed),
        resize_percent: RESIZE.load(Relaxed) as u8,
        save_original: SAVE_ORIGINAL.load(Relaxed),
        save_avenged: SAVE_AVENGED.load(Relaxed),
        save_folder: SAVE_FOLDER
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder_failures: FOLDER_FAILURES.load(Relaxed),
    };
    if let Err(error) = std::fs::write(home().join("settings.ini"), settings.to_ini()) {
        log(&format!("Could not save settings: {}", error));
    }
}
fn load_settings() -> Settings {
    read_settings(&home().join("settings.ini"))
}
fn read_settings(path: &std::path::Path) -> Settings {
    std::fs::read_to_string(path)
        .map(|text| Settings::from_ini(&text))
        .unwrap_or_default()
}
unsafe fn bytes(fmt: u32) -> Option<Vec<u8>> {
    let h = GetClipboardData(fmt);
    if h.is_null() {
        return None;
    }
    let n = GlobalSize(h);
    if n == 0 || n > 512 * 1024 * 1024 {
        return None;
    }
    let p = GlobalLock(h);
    if p.is_null() {
        return None;
    }
    let b = std::slice::from_raw_parts(p as *const u8, n).to_vec();
    GlobalUnlock(h);
    Some(b)
}
#[derive(Clone)]
struct ScanOptions {
    quality: u8,
    compression: Compression,
    automatic_webp_to_png: bool,
    resize_percent: u8,
    save_original: bool,
    save_avenged: bool,
    folder: Option<PathBuf>,
    revision: u32,
}
impl ScanOptions {
    fn current() -> Self {
        Self {
            quality: QUALITY.load(Relaxed) as u8,
            compression: Compression::from_id(COMPRESSION.load(Relaxed)),
            automatic_webp_to_png: AUTOMATIC_WEBP_TO_PNG.load(Relaxed),
            resize_percent: RESIZE.load(Relaxed) as u8,
            save_original: SAVE_ORIGINAL.load(Relaxed),
            save_avenged: SAVE_AVENGED.load(Relaxed),
            folder: SAVE_FOLDER.lock().unwrap().clone(),
            revision: REVISION.load(Relaxed),
        }
    }
    fn active(&self) -> bool {
        ENABLED.load(Relaxed) && self.revision == REVISION.load(Relaxed)
    }
    fn for_file(&self, generated: bool) -> Self {
        let mut options = self.clone();
        if generated {
            // A working copy already has the selected resize applied. A WebP
            // compatibility upgrade changes its format, not its size again.
            options.resize_percent = 100;
        }
        options
    }
}
fn options_changed() {
    REVISION.fetch_add(1, AcqRel);
    LAST.store(0, Relaxed);
    *PENDING.lock().unwrap() = None;
    save();
}
fn cache_key(data: &[u8], dib: bool, quality: u8, resize: u8) -> String {
    format!(
        "v4-{:x}-{}-q{}-r{}",
        Sha256::digest(data),
        dib,
        quality,
        resize
    )
}
fn failed_folder_attempt(folder: &mut Option<PathBuf>, failures: &mut u32) {
    *failures = failures.saturating_add(1);
    if *failures >= 10 {
        *folder = None;
        *failures = 0;
    }
}
fn save_with_fallback(
    custom: Option<&std::path::Path>,
    default: &std::path::Path,
    mut write: impl FnMut(&std::path::Path) -> std::io::Result<()>,
) -> bool {
    if let Some(folder) = custom {
        if write(folder).is_ok() {
            return false;
        }
        let _ = write(default);
        return true;
    }
    let _ = write(default);
    false
}
/// Count one failed custom-folder attempt for each image, including when both
/// Original and Avenged are selected. Fallback errors are deliberately silent.
fn archive_images(
    data: &[u8],
    dib: bool,
    img: &image::DynamicImage,
    out: Option<&imaging::Encoded>,
    key: &str,
    options: &ScanOptions,
) {
    if !options.active() {
        return;
    }
    if !options.save_original && (!options.save_avenged || out.is_none()) {
        return;
    }
    let write = |root: &std::path::Path| -> std::io::Result<()> {
        if !options.active() {
            return Ok(());
        }
        if options.save_original {
            storage::save_original(root, data, dib, img)?;
        }
        if options.save_avenged && options.active() {
            if let Some(out) = out {
                storage::save_avenged(root, key, &out.bytes, out.extension)?;
            }
        }
        Ok(())
    };
    let custom = options
        .folder
        .as_ref()
        .filter(|folder| SAVE_FOLDER.lock().unwrap().as_ref() == Some(*folder));
    let failed = save_with_fallback(
        custom.map(PathBuf::as_path),
        &storage::default_save_folder(),
        write,
    );
    if failed {
        let folder = custom.unwrap();
        {
            let mut current = SAVE_FOLDER.lock().unwrap();
            if current.as_ref() == Some(folder) {
                let mut failures = FOLDER_FAILURES.load(Relaxed);
                failed_folder_attempt(&mut current, &mut failures);
                FOLDER_FAILURES.store(failures, Relaxed);
            }
        }
        save();
    }
}
fn convert(data: &[u8], dib: bool, options: &ScanOptions) -> Option<PathBuf> {
    let key = format!(
        "{}-c{}-w{}",
        cache_key(data, dib, options.quality, options.resize_percent),
        options.compression as u32,
        options.automatic_webp_to_png
    );
    // Archiving preferences also affect scan results, including a no-shrink
    // image whose original still needs saving after Save Original is enabled.
    let memo_key = format!(
        "{}-{}-{}-{:x}",
        key,
        options.save_original,
        options.save_avenged,
        Sha256::digest(
            options
                .folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
                .as_bytes()
        )
    );
    {
        let guard = CACHE.lock().unwrap();
        if let Some(c) = guard.as_ref() {
            if let Some(v) = c.map.get(&memo_key) {
                if v.is_none() || v.as_ref().is_some_and(|p| p.exists()) {
                    return v.clone();
                }
            }
        }
    }
    let result = (|| {
        let source = if dib {
            image::ImageFormat::Bmp
        } else {
            image::guess_format(data).ok()?
        };
        let img = if dib {
            dib_image(data)?
        } else if source == image::ImageFormat::WebP {
            let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(data)).ok()?;
            if decoder.has_animation() {
                return None;
            }
            image::DynamicImage::from_decoder(decoder).ok()?
        } else {
            if data.starts_with(b"GIF8")
                || (source == image::ImageFormat::Png && data.windows(4).any(|w| w == b"acTL"))
            {
                return None;
            }
            image::load_from_memory(data).ok()?
        };
        let out = imaging::encoded_as(
            &img,
            data.len(),
            options.quality,
            options.resize_percent,
            options.compression,
            source,
            options.automatic_webp_to_png,
        );
        if !options.active() {
            return None;
        }
        archive_images(data, dib, &img, out.as_ref(), &key, options);
        let out = out?;
        let path = image_dir()
            .join("Clipboard")
            .join(format!("{}.{}", key, out.extension));
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, &out.bytes).ok()?;
        log(&format!(
            "Processed {} bytes to {} bytes ({}, resize {}%)",
            data.len(),
            out.bytes.len(),
            out.extension,
            options.resize_percent
        ));
        Some(path)
    })();
    if !options.active() {
        return None;
    }
    let mut guard = CACHE.lock().unwrap();
    let c = guard.get_or_insert_with(|| Cache {
        map: HashMap::new(),
        order: VecDeque::new(),
    });
    if !c.map.contains_key(&memo_key) {
        if c.order.len() >= 256 {
            if let Some(k) = c.order.pop_front() {
                c.map.remove(&k);
            }
        }
        c.order.push_back(memo_key.clone());
    }
    c.map.insert(memo_key, result.clone());
    result
}
#[cfg(test)]
fn encoded(img: &image::DynamicImage, original: usize, quality: u8) -> Option<imaging::Encoded> {
    imaging::encoded(img, original, quality, 100)
}
struct BusyGuard {
    revision: u32,
}
impl Drop for BusyGuard {
    fn drop(&mut self) {
        // A menu change during encoding must let the new setting scan again,
        // including scans that decide the encoded image would be larger.
        if self.revision != REVISION.load(Relaxed) {
            LAST.store(0, Relaxed);
        }
        BUSY.store(false, Release);
    }
}
fn eligible_file(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    [
        "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "ico",
    ]
    .contains(&ext.as_str())
}
fn convert_generated_webp(path: &std::path::Path, automatic_webp_to_png: bool) -> bool {
    // Old releases (or Same as Source with conversion OFF) can leave WebP
    // working copies. Convert those once; continue skipping our JPG/PNG output.
    automatic_webp_to_png
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("webp"))
}
fn log(message: &str) {
    use std::io::Write;
    let path = home().join("activity.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 262144) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            file,
            "{}: {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            message
        );
    }
}
unsafe fn scan(hwnd: HWND, seq: u32, options: ScanOptions) {
    let revision = options.revision;
    let _busy = BusyGuard { revision };
    if OpenClipboard(hwnd) == 0 {
        return;
    }
    let mut paths = Vec::new();
    let mut raw = None;
    if IsClipboardFormatAvailable(15) != 0 {
        let h = GetClipboardData(15);
        let count = DragQueryFileW(h, 0xffffffff, null_mut(), 0);
        for i in 0..count {
            let n = DragQueryFileW(h, i, null_mut(), 0);
            let mut buf = vec![0; n as usize + 1];
            DragQueryFileW(h, i, buf.as_mut_ptr(), n + 1);
            paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])));
        }
    } else {
        let png = RegisterClipboardFormatW(wide("PNG").as_ptr());
        let jpeg = RegisterClipboardFormatW(wide("JFIF").as_ptr());
        // Some applications offer encoded WebP as well as a decoded PNG/DIB.
        // Prefer the real source so the conversion rule and size comparison
        // use WebP bytes. Validate custom formats before accepting them.
        let webp = ["WebP", "image/webp"].into_iter().find_map(|name| {
            bytes(RegisterClipboardFormatW(wide(name).as_ptr()))
                .filter(|data| image::guess_format(data).ok() == Some(image::ImageFormat::WebP))
        });
        // Prefer alpha-preserving encoded data over synthesized bitmap formats.
        raw = webp
            .or_else(|| bytes(png))
            .or_else(|| bytes(jpeg))
            .map(|b| (b, false));
        if raw.is_none() {
            raw = bytes(17).or_else(|| bytes(8)).map(|b| (b, true));
        }
    }
    let upgrade_own_files = paths.iter().any(|p| p.starts_with(image_dir()))
        && (IsClipboardFormatAvailable(RegisterClipboardFormatW(wide("FileContents").as_ptr()))
            == 0
            || (paths.len() == 1 && IsClipboardFormatAvailable(8) == 0));
    let captured = GetClipboardSequenceNumber();
    CloseClipboard();
    if captured != seq {
        return;
    }
    LAST.store(seq, Relaxed);
    let mut changed = upgrade_own_files;
    if !paths.is_empty() {
        for path in &mut paths {
            if path.starts_with(legacy_home().join("Images")) {
                // Upgrade clipboard history entries made by older releases.
                if let Some(name) = path.file_name() {
                    let target = image_dir().join("Clipboard").join(name);
                    let _ = std::fs::create_dir_all(target.parent().unwrap());
                    if std::fs::copy(&path, &target).is_ok() {
                        *path = target;
                        changed = true;
                    }
                }
                if !convert_generated_webp(path, options.automatic_webp_to_png) {
                    continue;
                }
            }
            let generated = path.starts_with(image_dir())
                || path.starts_with(image_dir().parent().unwrap().join("Desktop Image Avenger"))
                || path.starts_with(legacy_home().join("Images"));
            if generated && !convert_generated_webp(path, options.automatic_webp_to_png) {
                continue;
            }

            if !eligible_file(path) {
                continue;
            }
            if let Ok(meta) = path.metadata() {
                if meta.len() > 512 * 1024 * 1024 {
                    continue;
                }
            } else {
                continue;
            }
            if let Ok(data) = std::fs::read(&path) {
                if let Some(new) = convert(&data, false, &options.for_file(generated)) {
                    *path = new;
                    changed = true;
                }
            }
        }
    } else if let Some((data, dib)) = raw {
        if let Some(path) = convert(&data, dib, &options) {
            paths.push(path);
            changed = true;
        }
    }
    if !changed || !ENABLED.load(Relaxed) || revision != REVISION.load(Relaxed) {
        return;
    }
    match ole_clipboard::prepare(&paths) {
        Ok(prepared) => {
            let pending = Box::into_raw(Box::new((seq, revision, prepared)));
            if PostMessageW(hwnd, WM_APP + 1, 0, pending as isize) == 0 {
                drop(Box::from_raw(pending));
            }
        }
        Err(error) => log(&format!("Could not prepare clipboard contents: {}", error)),
    }
}
unsafe fn try_publish() {
    let mut pending = PENDING.lock().unwrap();
    let Some((seq, revision, prepared, attempts)) = pending.as_mut() else {
        return;
    };
    if *revision != REVISION.load(Relaxed) {
        LAST.store(0, Relaxed);
        *pending = None;
        return;
    }
    if !ENABLED.load(Relaxed) || GetClipboardSequenceNumber() != *seq {
        *pending = None;
        return;
    }
    match ole_clipboard::publish(prepared) {
        Ok(()) => {
            LAST.store(GetClipboardSequenceNumber(), Relaxed);
            log("Published clipboard attachments with contents");
            *pending = None;
        }
        Err(hr) => {
            *attempts += 1;
            log(&format!(
                "Clipboard publication attempt {} failed: HRESULT 0x{:08X}; will retry",
                attempts, hr as u32
            ));
            if *attempts >= 20 {
                *pending = None;
                LAST.store(0, Relaxed);
            }
        }
    }
}
unsafe fn tray_menu() -> HMENU {
    let menu = CreatePopupMenu();
    let a = if ENABLED.load(Relaxed) {
        "Turn Off"
    } else {
        "Turn On"
    };
    AppendMenuW(menu, MF_STRING, 1, wide(a).as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
    let compression_menu = CreatePopupMenu();
    for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
        AppendMenuW(
            compression_menu,
            MF_STRING
                | if COMPRESSION.load(Relaxed) == compression as u32 {
                    MF_CHECKED
                } else {
                    0
                },
            (COMPRESSION_MENU_BASE + compression as u32) as usize,
            wide(compression.label()).as_ptr(),
        );
    }
    AppendMenuW(compression_menu, MF_SEPARATOR, 0, null_mut());
    let quality_menu = CreatePopupMenu();
    let selected = QUALITY.load(Relaxed);
    for quality in (50..=100).step_by(5) {
        AppendMenuW(
            quality_menu,
            MF_STRING | if quality == selected { MF_CHECKED } else { 0 },
            (QUALITY_MENU_BASE + quality) as usize,
            wide(&format!("{}%", quality)).as_ptr(),
        );
    }
    AppendMenuW(
        compression_menu,
        MF_POPUP,
        quality_menu as usize,
        wide(&format!("JPEG quality: {}%", selected)).as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_POPUP,
        compression_menu as usize,
        wide("Compression").as_ptr(),
    );
    let resize_menu = CreatePopupMenu();
    let selected_resize = RESIZE.load(Relaxed);
    for resize in [100, 75, 50, 25] {
        let label = if resize == 100 {
            "Don't".to_owned()
        } else {
            format!("{}%", resize)
        };
        AppendMenuW(
            resize_menu,
            MF_STRING
                | if resize == selected_resize {
                    MF_CHECKED
                } else {
                    0
                },
            (RESIZE_MENU_BASE + resize) as usize,
            wide(&label).as_ptr(),
        );
    }
    AppendMenuW(
        menu,
        MF_POPUP,
        resize_menu as usize,
        wide("Resize").as_ptr(),
    );
    let misc_menu = CreatePopupMenu();
    AppendMenuW(
        misc_menu,
        MF_STRING
            | if AUTOMATIC_WEBP_TO_PNG.load(Relaxed) {
                MF_CHECKED
            } else {
                0
            },
        AUTOMATIC_WEBP_MENU as usize,
        wide("Automatic WebP to PNG conversion").as_ptr(),
    );
    AppendMenuW(menu, MF_POPUP, misc_menu as usize, wide("Misc").as_ptr());
    let save_menu = CreatePopupMenu();
    AppendMenuW(
        save_menu,
        MF_STRING
            | if SAVE_ORIGINAL.load(Relaxed) {
                MF_CHECKED
            } else {
                0
            },
        4,
        wide("Save Original").as_ptr(),
    );
    AppendMenuW(
        save_menu,
        MF_STRING
            | if SAVE_AVENGED.load(Relaxed) {
                MF_CHECKED
            } else {
                0
            },
        5,
        wide("Save Avenged").as_ptr(),
    );
    let folder_label = SAVE_FOLDER
        .lock()
        .unwrap()
        .as_ref()
        .map(|folder| format!("... ({})", folder.to_string_lossy().replace('&', "&&")))
        .unwrap_or_else(|| "...".to_owned());
    AppendMenuW(save_menu, MF_STRING, 6, wide(&folder_label).as_ptr());
    AppendMenuW(menu, MF_POPUP, save_menu as usize, wide("Save").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
    AppendMenuW(
        menu,
        MF_STRING
            | if START_WITH_WINDOWS.load(Relaxed) {
                MF_CHECKED
            } else {
                0
            },
        3,
        wide("Start with Windows").as_ptr(),
    );
    AppendMenuW(menu, MF_STRING, 2, wide("Exit").as_ptr());
    menu
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_APP + 1 {
        let (seq, revision, prepared) =
            *Box::from_raw(l as *mut (u32, u32, ole_clipboard::Prepared));
        if revision != REVISION.load(Relaxed) {
            LAST.store(0, Relaxed);
        } else if ENABLED.load(Relaxed) && GetClipboardSequenceNumber() == seq {
            *PENDING.lock().unwrap() = Some((seq, revision, prepared, 0));
            try_publish();
        }
        return 0;
    }
    match msg {
        WM_TIMER => {
            try_publish();
            let seq = GetClipboardSequenceNumber();
            if ENABLED.load(Relaxed)
                && PENDING.lock().unwrap().is_none()
                && seq != LAST.load(Relaxed)
                && BUSY.compare_exchange(false, true, AcqRel, Relaxed).is_ok()
            {
                let handle = hwnd as usize;
                let options = ScanOptions::current();
                std::thread::spawn(move || {
                    let _ = std::panic::catch_unwind(|| scan(handle as HWND, seq, options));
                });
            }
            0
        }
        WM_APP => {
            if l as u32 == WM_LBUTTONUP {
                toggle_enabled(hwnd);
                return 0;
            }
            if l as u32 == WM_RBUTTONUP || l as u32 == WM_CONTEXTMENU {
                let menu = tray_menu();
                let mut p = std::mem::zeroed();
                GetCursorPos(&mut p);
                SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    p.x,
                    p.y,
                    0,
                    hwnd,
                    null_mut(),
                );
                DestroyMenu(menu);
                match cmd {
                    1 => {
                        toggle_enabled(hwnd);
                    }
                    2 => {
                        DestroyWindow(hwnd);
                    }
                    3 => {
                        let enabled = !START_WITH_WINDOWS.load(Relaxed);
                        match startup::set_enabled(enabled) {
                            Ok(()) => {
                                START_WITH_WINDOWS.store(enabled, Relaxed);
                                save();
                            }
                            Err(error) => {
                                let message =
                                    format!("Could not change Start with Windows: {}", error);
                                log(&message);
                                MessageBoxW(
                                    hwnd,
                                    wide(&message).as_ptr(),
                                    wide("Clipboard Avenger").as_ptr(),
                                    MB_OK | MB_ICONERROR,
                                );
                            }
                        }
                    }
                    4 => {
                        SAVE_ORIGINAL.fetch_xor(true, Relaxed);
                        options_changed();
                    }
                    5 => {
                        SAVE_AVENGED.fetch_xor(true, Relaxed);
                        options_changed();
                    }
                    6 => {
                        let current = SAVE_FOLDER
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(storage::default_save_folder);
                        if let Some(folder) = storage::choose_folder(hwnd, &current) {
                            let is_default = folder == storage::default_save_folder();
                            *SAVE_FOLDER.lock().unwrap() =
                                if is_default { None } else { Some(folder) };
                            FOLDER_FAILURES.store(0, Relaxed);
                            options_changed();
                        }
                    }
                    cmd if cmd as u32 == AUTOMATIC_WEBP_MENU => {
                        AUTOMATIC_WEBP_TO_PNG.fetch_xor(true, Relaxed);
                        options_changed();
                    }
                    cmd if [
                        COMPRESSION_MENU_BASE,
                        COMPRESSION_MENU_BASE + 1,
                        COMPRESSION_MENU_BASE + 3,
                    ]
                    .contains(&(cmd as u32)) =>
                    {
                        COMPRESSION.store(cmd as u32 - COMPRESSION_MENU_BASE, Relaxed);
                        options_changed();
                        update_tooltip(hwnd);
                    }
                    cmd if (RESIZE_MENU_BASE + 25..=RESIZE_MENU_BASE + 100)
                        .contains(&(cmd as u32)) =>
                    {
                        let resize = cmd as u32 - RESIZE_MENU_BASE;
                        if [100, 75, 50, 25].contains(&resize) {
                            RESIZE.store(resize, Relaxed);
                            options_changed();
                        }
                    }
                    cmd if (QUALITY_MENU_BASE + 50..=QUALITY_MENU_BASE + 100)
                        .contains(&(cmd as u32)) =>
                    {
                        let quality = (cmd as u32 - QUALITY_MENU_BASE) as u8;
                        if valid_quality(quality) {
                            QUALITY.store(quality as u32, Relaxed);
                            options_changed();
                            update_tooltip(hwnd);
                        }
                    }
                    _ => {}
                }
            }
            0
        }
        WM_DESTROY => {
            let mut n: NOTIFYICONDATAW = std::mem::zeroed();
            n.cbSize = std::mem::size_of_val(&n) as u32;
            n.hWnd = hwnd;
            n.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &n);
            let icons = tray_icons();
            if icons.1 != 0 {
                DestroyIcon(icons.1 as HICON);
            }
            OleFlushClipboard();
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
unsafe fn update_tooltip(hwnd: HWND) {
    let mut n: NOTIFYICONDATAW = std::mem::zeroed();
    n.cbSize = std::mem::size_of_val(&n) as u32;
    n.hWnd = hwnd;
    n.uID = 1;
    n.uFlags = NIF_TIP | NIF_ICON;
    n.hIcon = selected_tray_icon();
    let tip = tray_tip();
    n.szTip[..tip.len()].copy_from_slice(&tip);
    Shell_NotifyIconW(NIM_MODIFY, &n);
}
fn tray_tip() -> Vec<u16> {
    wide(&format!(
        "Clipboard Avenger — {} — {} / JPEG {}%",
        if ENABLED.load(Relaxed) { "ON" } else { "OFF" },
        Compression::from_id(COMPRESSION.load(Relaxed)).label(),
        QUALITY.load(Relaxed)
    ))
}
unsafe fn toggle_enabled(hwnd: HWND) {
    ENABLED.fetch_xor(true, Relaxed);
    options_changed();
    update_tooltip(hwnd);
}
fn tray_icons() -> &'static (usize, usize) {
    static ICONS: OnceLock<(usize, usize)> = OnceLock::new();
    ICONS.get_or_init(|| unsafe {
        let instance = GetModuleHandleW(null_mut());
        let normal = LoadIconW(instance, 1usize as *const u16);
        // Derive a second icon from the existing artwork, preserving its alpha.
        let dark = dark_tray_icon(instance);
        (normal as usize, dark as usize)
    })
}
unsafe fn selected_tray_icon() -> HICON {
    let icons = tray_icons();
    if ENABLED.load(Relaxed) || icons.1 == 0 {
        icons.0 as HICON
    } else {
        icons.1 as HICON
    }
}
unsafe fn dark_tray_icon(_instance: HINSTANCE) -> HICON {
    let Ok(image) = image::load_from_memory(include_bytes!("../clipboard-avenger.ico")) else {
        return null_mut();
    };
    let mut rgba = image
        .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    for p in rgba.pixels_mut() {
        for channel in &mut p.0[..3] {
            *channel = (u16::from(*channel) * 35 / 100) as u8;
        }
    }
    let mut ico = Vec::new();
    use image::ImageEncoder;
    if image::codecs::ico::IcoEncoder::new(&mut ico)
        .write_image(rgba.as_raw(), 32, 32, image::ExtendedColorType::Rgba8)
        .is_err()
    {
        return null_mut();
    }
    if ico.len() < 22 {
        return null_mut();
    }
    let offset = u32::from_le_bytes(ico[18..22].try_into().unwrap()) as usize;
    if offset >= ico.len() {
        return null_mut();
    }
    CreateIconFromResourceEx(
        ico[offset..].as_ptr(),
        (ico.len() - offset) as u32,
        1,
        0x00030000,
        32,
        32,
        LR_DEFAULTCOLOR,
    )
}
fn main() {
    unsafe {
        if OleInitialize(null_mut()) < 0 {
            return;
        }
        let first_named_launch = !home().exists();
        let _ = std::fs::create_dir_all(home());
        if first_named_launch && legacy_home().join("settings.ini").is_file() {
            let _ = std::fs::copy(
                legacy_home().join("settings.ini"),
                home().join("settings.ini"),
            );
        }
        let _ = std::fs::create_dir_all(image_dir());
        let mutex = CreateMutexW(
            null_mut(),
            0,
            wide("Local\\DesktopImageAvenger.Singleton").as_ptr(),
        );
        if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let settings = load_settings();
        ENABLED.store(settings.enabled, Relaxed);
        QUALITY.store(settings.jpeg_quality as u32, Relaxed);
        COMPRESSION.store(settings.compression as u32, Relaxed);
        AUTOMATIC_WEBP_TO_PNG.store(settings.automatic_webp_to_png, Relaxed);
        RESIZE.store(settings.resize_percent as u32, Relaxed);
        SAVE_ORIGINAL.store(settings.save_original, Relaxed);
        SAVE_AVENGED.store(settings.save_avenged, Relaxed);
        *SAVE_FOLDER.lock().unwrap() = if settings.save_folder.is_empty() {
            None
        } else {
            Some(PathBuf::from(settings.save_folder))
        };
        FOLDER_FAILURES.store(settings.folder_failures, Relaxed);
        match startup::set_enabled(settings.start_with_windows) {
            Ok(()) => START_WITH_WINDOWS.store(settings.start_with_windows, Relaxed),
            Err(error) => log(&format!("Could not restore Start with Windows: {}", error)),
        }
        save();
        let instance = GetModuleHandleW(null_mut());
        let cls = wide("ClipboardAvenger");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.hInstance = instance;
        wc.lpszClassName = cls.as_ptr();
        wc.lpfnWndProc = Some(proc);
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            cls.as_ptr(),
            cls.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        let mut n: NOTIFYICONDATAW = std::mem::zeroed();
        n.cbSize = std::mem::size_of_val(&n) as u32;
        n.hWnd = hwnd;
        n.uID = 1;
        n.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        n.uCallbackMessage = WM_APP;
        n.hIcon = selected_tray_icon();
        let tip = tray_tip();
        n.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(NIM_ADD, &n);
        SetTimer(hwnd, 1, 250, None);
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        CloseHandle(mutex);
        OleUninitialize();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_transparent_pixel_uses_lossless_png() {
        let mut rgba = image::RgbaImage::from_pixel(100, 100, image::Rgba([27, 110, 42, 255]));
        rgba.put_pixel(12, 34, image::Rgba([27, 110, 42, 254]));
        let im = DynamicImage::ImageRgba8(rgba.clone());
        let out = encoded(&im, 40000, 95).unwrap();
        assert_eq!(out.extension, "png");
        assert_eq!(
            image::load_from_memory(&out.bytes).unwrap().to_rgba8(),
            rgba
        );
        assert!(out.bytes.len() < 40000);
        assert!(encoded(&im, 1, 95).is_none());
        for quality in [50, 100] {
            let out = encoded(&im, 40000, quality).unwrap();
            assert_eq!(out.extension, "png");
            assert_eq!(
                image::load_from_memory(&out.bytes).unwrap().to_rgba8(),
                rgba
            );
        }
    }
    #[test]
    fn opaque_rgba_is_jpeg_even_below_old_threshold() {
        let im = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            100,
            100,
            image::Rgba([27, 110, 42, 255]),
        ));
        let out = encoded(&im, 40000, 95).unwrap();
        assert_eq!(out.extension, "jpg");
        assert_eq!(&out.bytes[..2], &[255, 216]);
        assert!(encoded(&im, out.bytes.len(), 95).is_none());
    }
    #[test]
    fn jpeg_larger_than_source_is_skipped() {
        let mut seed = 1u32;
        let im = image::RgbImage::from_fn(1400, 1400, |_, _| {
            let mut c = [0; 3];
            for v in &mut c {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = (seed >> 24) as u8;
            }
            image::Rgb(c)
        });
        assert!(encoded(&DynamicImage::ImageRgb8(im), 1_000_000, 95).is_none());
    }
    #[test]
    fn files_always_eligible_but_nonimages_ignored() {
        for name in ["a.png", "a.jpg", "a.webp", "a.bmp"] {
            assert!(eligible_file(std::path::Path::new(name)));
        }
        assert!(!eligible_file(std::path::Path::new("a.txt")));
        assert!(dib_image(&[0; 40]).is_none());
    }
    #[test]
    fn jpeg_quality_changes_output_and_cache_identity() {
        let mut seed = 1u32;
        let im = DynamicImage::ImageRgb8(image::RgbImage::from_fn(128, 128, |_, _| {
            let mut rgb = [0; 3];
            for v in &mut rgb {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = (seed >> 24) as u8;
            }
            image::Rgb(rgb)
        }));
        let low = encoded(&im, usize::MAX, 50).unwrap();
        let high = encoded(&im, usize::MAX, 100).unwrap();
        assert!(low.bytes.len() < high.bytes.len());
        for out in [low, high] {
            let decoded = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (128, 128));
        }
        assert_ne!(
            cache_key(b"same image", false, 50, 100),
            cache_key(b"same image", false, 100, 100)
        );
    }
    #[test]
    fn ini_remembers_all_settings_for_every_quality() {
        for enabled in [false, true] {
            for jpeg_quality in (50..=100).step_by(5) {
                let settings = Settings {
                    enabled,
                    jpeg_quality,
                    start_with_windows: false,
                    ..Settings::default()
                };
                assert_eq!(Settings::from_ini(&settings.to_ini()), settings);
                let with_startup = Settings {
                    start_with_windows: true,
                    ..settings
                };
                assert_eq!(Settings::from_ini(&with_startup.to_ini()), with_startup);
            }
        }
        for value in ["0", "49", "51", "101", "255", "invalid"] {
            let settings = Settings::from_ini(&format!(
                "[Settings]\nEnabled=false\nJpegQuality={}\n",
                value
            ));
            assert!(!settings.enabled);
            assert_eq!(settings.jpeg_quality, 95);
        }
        assert_eq!(Settings::from_ini(""), Settings::default());
    }
    #[test]
    fn missing_ini_defaults_to_on_and_95_even_with_legacy_settings() {
        let dir = std::env::temp_dir().join(format!(
            "avenger-settings-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("settings"), "false").unwrap();
        let result = read_settings(&dir.join("settings.ini"));
        std::fs::remove_file(dir.join("settings")).unwrap();
        std::fs::remove_dir(dir).unwrap();
        assert_eq!(
            result,
            Settings {
                enabled: true,
                jpeg_quality: 95,
                start_with_windows: false,
                save_original: false,
                save_avenged: false,
                resize_percent: 100,
                ..Settings::default()
            }
        );
    }
    #[test]
    fn all_new_settings_and_folder_characters_round_trip() {
        for resize_percent in [100, 75, 50, 25] {
            let settings = Settings {
                resize_percent,
                save_original: false,
                save_avenged: false,
                save_folder: "C:\\Pictures\\Images #1; saved".into(),
                folder_failures: 9,
                ..Settings::default()
            };
            assert_eq!(Settings::from_ini(&settings.to_ini()), settings);
        }
        assert_ne!(
            cache_key(b"same", false, 95, 100),
            cache_key(b"same", false, 95, 50)
        );
        let exhausted =
            Settings::from_ini("[Settings]\nSaveFolder=C:\\Unavailable\nSaveFolderFailures=10\n");
        assert!(exhausted.save_folder.is_empty());
        assert_eq!(exhausted.folder_failures, 0);
    }
    #[test]
    fn tray_menu_matches_requested_order_and_submenus() {
        unsafe {
            let menu = tray_menu();
            assert!(!menu.is_null());
            assert_eq!(GetMenuItemCount(menu), 9);
            assert_eq!(GetMenuItemID(menu, 0), 1);
            assert_ne!(GetMenuState(menu, 1, MF_BYPOSITION) & MF_SEPARATOR, 0);
            assert_ne!(GetMenuState(menu, 6, MF_BYPOSITION) & MF_SEPARATOR, 0);
            assert_eq!(GetMenuItemID(menu, 7), 3);
            assert_eq!(GetMenuItemID(menu, 8), 2);
            let compression = GetSubMenu(menu, 2);
            assert_eq!(GetMenuItemCount(compression), 5);
            for (index, id) in [0, 1, 3].into_iter().enumerate() {
                assert_eq!(
                    GetMenuItemID(compression, index as i32),
                    COMPRESSION_MENU_BASE + id
                );
            }
            assert_ne!(
                GetMenuState(compression, 3, MF_BYPOSITION) & MF_SEPARATOR,
                0
            );
            assert_eq!(GetMenuItemCount(GetSubMenu(compression, 4)), 11);
            let misc = GetSubMenu(menu, 4);
            assert_eq!(GetMenuItemCount(misc), 1);
            assert_eq!(GetMenuItemID(misc, 0), AUTOMATIC_WEBP_MENU);
            assert_ne!(GetMenuState(misc, 0, MF_BYPOSITION) & MF_CHECKED, 0);
            let resize = GetSubMenu(menu, 3);
            assert_eq!(GetMenuItemCount(resize), 4);
            for (index, percent) in [100, 75, 50, 25].into_iter().enumerate() {
                assert_eq!(
                    GetMenuItemID(resize, index as i32),
                    RESIZE_MENU_BASE + percent
                );
            }
            let save = GetSubMenu(menu, 5);
            assert_eq!(GetMenuItemCount(save), 3);
            assert_eq!(GetMenuItemID(save, 0), 4);
            assert_eq!(GetMenuItemID(save, 1), 5);
            assert_eq!(GetMenuItemID(save, 2), 6);
            assert_eq!(GetMenuState(save, 0, MF_BYPOSITION) & MF_CHECKED, 0);
            assert_eq!(GetMenuState(save, 1, MF_BYPOSITION) & MF_CHECKED, 0);
            DestroyMenu(menu);
        }
    }
    #[test]
    fn every_compression_choice_is_saved_and_restored() {
        for compression in [Compression::Png, Compression::Jpg, Compression::Source] {
            let settings = Settings {
                compression,
                ..Settings::default()
            };
            assert_eq!(Settings::from_ini(&settings.to_ini()), settings);
        }
        assert_eq!(
            Settings::from_ini("[Settings]\nCompression=unknown\n").compression,
            Compression::Jpg
        );
        assert_eq!(
            Settings::from_ini("[Settings]\nCompression=WEBP\n").compression,
            Compression::Jpg
        );
    }
    #[test]
    fn automatic_webp_conversion_is_remembered_and_defaults_on() {
        assert!(Settings::default().automatic_webp_to_png);
        assert!(Settings::from_ini("[Settings]\nEnabled=false\n").automatic_webp_to_png);
        for automatic_webp_to_png in [false, true] {
            let settings = Settings {
                automatic_webp_to_png,
                ..Settings::default()
            };
            assert_eq!(Settings::from_ini(&settings.to_ini()), settings);
        }
        assert!(!Settings::from_ini("[Settings]\nAutomaticWebpToPng=0\n").automatic_webp_to_png);
        assert!(
            Settings::from_ini("[Settings]\nAutomaticWebpToPng=invalid\n").automatic_webp_to_png
        );
    }
    #[test]
    fn generated_webp_can_be_converted_without_reprocessing_our_jpg_or_png() {
        for name in ["generated.webp", "generated.WEBP"] {
            assert!(convert_generated_webp(std::path::Path::new(name), true));
            assert!(!convert_generated_webp(std::path::Path::new(name), false));
        }
        for name in ["generated.jpg", "generated.png", "not-an-image.txt"] {
            assert!(!convert_generated_webp(std::path::Path::new(name), true));
        }
    }
    #[test]
    fn generated_webp_format_upgrade_does_not_resize_a_second_time() {
        let original = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            48,
            32,
            image::Rgba([30, 120, 220, 127]),
        ));
        let mut webp = std::io::Cursor::new(Vec::new());
        original
            .write_to(&mut webp, image::ImageFormat::WebP)
            .unwrap();
        let decoded = image::load_from_memory(webp.get_ref()).unwrap();
        let options = ScanOptions {
            quality: 95,
            compression: Compression::Jpg,
            automatic_webp_to_png: true,
            resize_percent: 50,
            save_original: false,
            save_avenged: false,
            folder: None,
            revision: 0,
        };
        for (generated, dimensions) in [(true, (48, 32)), (false, (24, 16))] {
            let options = options.for_file(generated);
            let out = imaging::encoded_as(
                &decoded,
                webp.get_ref().len(),
                options.quality,
                options.resize_percent,
                options.compression,
                image::ImageFormat::WebP,
                options.automatic_webp_to_png,
            )
            .unwrap();
            assert_eq!(out.extension, "png");
            let processed = image::load_from_memory(&out.bytes).unwrap();
            assert_eq!((processed.width(), processed.height()), dimensions);
            if generated {
                assert_eq!(processed.to_rgba8(), decoded.to_rgba8());
            }
        }
    }
    #[test]
    fn darker_tray_artwork_creates_a_valid_windows_icon() {
        unsafe {
            let icon = dark_tray_icon(GetModuleHandleW(null_mut()));
            assert!(!icon.is_null());
            let mut info: ICONINFO = std::mem::zeroed();
            assert_ne!(GetIconInfo(icon, &mut info), 0);
            assert!(!info.hbmColor.is_null());
            windows_sys::Win32::Graphics::Gdi::DeleteObject(info.hbmColor);
            windows_sys::Win32::Graphics::Gdi::DeleteObject(info.hbmMask);
            DestroyIcon(icon);
        }
    }
    #[test]
    fn failed_folder_falls_back_immediately_and_is_cleared_on_tenth_failure() {
        let custom = PathBuf::from("unavailable-custom-folder");
        let fallback = PathBuf::from("Saved");
        let mut folder = Some(custom.clone());
        let mut failures = 0;
        for attempt in 1..=10 {
            let mut visited = Vec::new();
            let failed = save_with_fallback(folder.as_deref(), &fallback, |path| {
                visited.push(path.to_path_buf());
                if path == custom {
                    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                } else {
                    Ok(())
                }
            });
            assert!(failed);
            assert_eq!(visited, [custom.clone(), fallback.clone()]);
            failed_folder_attempt(&mut folder, &mut failures);
            if attempt < 10 {
                assert_eq!(folder, Some(custom.clone()));
                assert_eq!(failures, attempt);
            }
        }
        assert_eq!(folder, None);
        assert_eq!(failures, 0);
        let mut visited = Vec::new();
        assert!(!save_with_fallback(folder.as_deref(), &fallback, |path| {
            visited.push(path.to_path_buf());
            Ok(())
        }));
        assert_eq!(visited, [fallback]);
    }
}
