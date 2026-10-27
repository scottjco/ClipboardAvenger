use image::{DynamicImage, ImageFormat};
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_void, OsString},
    fs::{self, OpenOptions},
    io::{self, Cursor, Write},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    System::Com::CoTaskMemFree,
    UI::{
        Shell::{BFFM_INITIALIZED, BFFM_SETSELECTIONW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS},
        WindowsAndMessaging::SendMessageW,
    },
};

pub fn default_save_folder() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Saved")
}

/// Save the original encoded bytes unchanged. A raw clipboard bitmap has no
/// portable file container, so archive its decoded pixels in a lossless PNG.
pub fn save_original(
    root: &Path,
    data: &[u8],
    dib: bool,
    img: &DynamicImage,
) -> io::Result<PathBuf> {
    let name = format!("original-{:x}", Sha256::digest(data));
    if dib {
        let mut bytes = Cursor::new(Vec::new());
        img.write_to(&mut bytes, ImageFormat::Png)
            .map_err(io::Error::other)?;
        save_unique(&root.join(format!("{name}.png")), bytes.get_ref())
    } else {
        let format = image::guess_format(data).map_err(io::Error::other)?;
        let extension = format.extensions_str().first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Unknown original image format")
        })?;
        save_unique(&root.join(format!("{name}.{extension}")), data)
    }
}

pub fn save_avenged(root: &Path, key: &str, bytes: &[u8], extension: &str) -> io::Result<PathBuf> {
    // The key is the app's content/settings hash, never a source filename.
    if key.is_empty()
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || !matches!(extension, "jpg" | "jpeg" | "png")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid archive filename",
        ));
    }
    save_unique(&root.join(format!("avenged-{key}.{extension}")), bytes)
}

fn save_unique(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(path.parent().expect("archive path has a parent"))?;
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(error);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if fs::read(path)? != bytes {
                // Never destroy an existing archive, including a file edited by
                // the user or left incomplete by a previous interrupted write.
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "An archive with this name already contains different data",
                ));
            }
        }
        Err(error) => return Err(error),
    }
    Ok(path.to_path_buf())
}

// These shell APIs require the optional Shell_Common windows-sys feature. Keep
// the small ABI definitions here so the picker needs no additional dependency.
#[repr(C)]
struct BrowseInfo {
    owner: HWND,
    root: *const c_void,
    display_name: *mut u16,
    title: *const u16,
    flags: u32,
    callback: Option<unsafe extern "system" fn(HWND, u32, LPARAM, LPARAM) -> i32>,
    parameter: LPARAM,
    image: i32,
}

#[link(name = "shell32")]
extern "system" {
    fn SHBrowseForFolderW(info: *const BrowseInfo) -> *mut c_void;
    fn SHGetPathFromIDListW(pidl: *const c_void, path: *mut u16) -> i32;
}

unsafe extern "system" fn browse_callback(
    dialog: HWND,
    message: u32,
    _parameter: LPARAM,
    initial_path: LPARAM,
) -> i32 {
    if message == BFFM_INITIALIZED {
        SendMessageW(dialog, BFFM_SETSELECTIONW, 1, initial_path);
    }
    0
}

/// Must be called on the app's OLE-initialized UI thread. The initial path and
/// title remain alive for the modal dialog, including its initialization call.
pub unsafe fn choose_folder(owner: HWND, current: &Path) -> Option<PathBuf> {
    let current: Vec<u16> = current.as_os_str().encode_wide().chain(Some(0)).collect();
    let title: Vec<u16> = "Choose where Clipboard Avenger saves images"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut display_name = [0u16; 260];
    let info = BrowseInfo {
        owner,
        root: null_mut(),
        display_name: display_name.as_mut_ptr(),
        title: title.as_ptr(),
        flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        callback: Some(browse_callback),
        parameter: current.as_ptr() as LPARAM,
        image: 0,
    };
    let pidl = SHBrowseForFolderW(&info);
    if pidl.is_null() {
        return None;
    }
    let mut path = [0u16; 260];
    let success = SHGetPathFromIDListW(pidl, path.as_mut_ptr());
    CoTaskMemFree(pidl);
    if success == 0 {
        return None;
    }
    let length = path.iter().position(|&character| character == 0)?;
    (length != 0).then(|| PathBuf::from(OsString::from_wide(&path[..length])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestFolder(PathBuf);
    impl TestFolder {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "clipboard-avenger-storage-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }
    impl Drop for TestFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn picture() -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(3, 2, |x, y| {
            Rgba([
                (x * 70) as u8,
                (y * 100) as u8,
                25,
                if x == 1 { 127 } else { 255 },
            ])
        }))
    }

    #[test]
    fn archives_encoded_original_exactly_once_and_keeps_variants_separate() {
        let folder = TestFolder::new();
        let img = picture();
        let mut buffer = Cursor::new(Vec::new());
        img.write_to(&mut buffer, ImageFormat::Png).unwrap();
        let bytes = buffer.into_inner();
        let original = save_original(&folder.0, &bytes, false, &img).unwrap();
        assert_eq!(original.extension().unwrap(), "png");
        assert_eq!(fs::read(&original).unwrap(), bytes);

        let mut permissions = fs::metadata(&original).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&original, permissions.clone()).unwrap();
        let again = save_original(&folder.0, &bytes, false, &img);
        permissions.set_readonly(false);
        fs::set_permissions(&original, permissions).unwrap();
        assert_eq!(again.unwrap(), original);

        let first = save_avenged(&folder.0, "content-q95-r100", b"first", "jpg").unwrap();
        let second = save_avenged(&folder.0, "content-q50-r75", b"second", "jpg").unwrap();
        for path in [&original, &first, &second] {
            assert_eq!(path.parent(), Some(folder.0.as_path()));
        }
        assert!(original
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("original-"));
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("avenged-"));
        assert_ne!(first, second);
        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        assert_eq!(fs::read(original).unwrap(), bytes);
    }

    #[test]
    fn bitmap_original_is_lossless_png_with_transparency() {
        let folder = TestFolder::new();
        let img = picture();
        let path = save_original(&folder.0, b"raw bitmap source", true, &img).unwrap();
        let restored = image::open(path).unwrap();
        assert_eq!(restored.to_rgba8(), img.to_rgba8());
    }

    #[test]
    fn avenged_archive_preserves_existing_data_and_rejects_path_components() {
        let folder = TestFolder::new();
        let path = save_avenged(&folder.0, "same-hash", b"original", "png").unwrap();
        assert_eq!(
            save_avenged(&folder.0, "same-hash", b"original", "png").unwrap(),
            path
        );
        assert!(save_avenged(&folder.0, "same-hash", b"changed", "png").is_err());
        assert_eq!(fs::read(path).unwrap(), b"original");
        assert!(save_avenged(&folder.0, "../outside", b"changed", "png").is_err());
    }
}
