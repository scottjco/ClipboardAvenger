//! Full OLE file clipboard. FileContents is indexed, so each attachment has
//! its own bytes, filename and size instead of only a CF_HDROP path.
use std::{
    ffi::c_void,
    path::PathBuf,
    ptr::null_mut,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use windows_sys::{
    core::{GUID, HRESULT},
    Win32::{
        Foundation::*,
        System::{Com::*, Memory::*, Ole::*},
    },
};
#[link(name = "shell32")]
extern "system" {
    fn SHCreateStdEnumFmtEtc(n: u32, formats: *const FORMATETC, out: *mut *mut c_void) -> HRESULT;
}
const FORMAT_ERROR: HRESULT = 0x80040064u32 as i32;
const UNSUPPORTED: HRESULT = 0x80004001u32 as i32;
const ADVISE_ERROR: HRESULT = 0x80040003u32 as i32;
#[repr(C)]
struct Vtable {
    query: unsafe extern "system" fn(*mut Object, *const GUID, *mut *mut c_void) -> HRESULT,
    add: unsafe extern "system" fn(*mut Object) -> u32,
    release: unsafe extern "system" fn(*mut Object) -> u32,
    get: unsafe extern "system" fn(*mut Object, *const FORMATETC, *mut STGMEDIUM) -> HRESULT,
    get_here: unsafe extern "system" fn(*mut Object, *const FORMATETC, *mut STGMEDIUM) -> HRESULT,
    query_get: unsafe extern "system" fn(*mut Object, *const FORMATETC) -> HRESULT,
    canonical: unsafe extern "system" fn(*mut Object, *const FORMATETC, *mut FORMATETC) -> HRESULT,
    set: unsafe extern "system" fn(*mut Object, *const FORMATETC, *const STGMEDIUM, i32) -> HRESULT,
    enumeration: unsafe extern "system" fn(*mut Object, u32, *mut *mut c_void) -> HRESULT,
    advise: unsafe extern "system" fn(
        *mut Object,
        *const FORMATETC,
        u32,
        *mut c_void,
        *mut u32,
    ) -> HRESULT,
    unadvise: unsafe extern "system" fn(*mut Object, u32) -> HRESULT,
    enum_advise: unsafe extern "system" fn(*mut Object, *mut *mut c_void) -> HRESULT,
}
struct Entry {
    format: u16,
    index: i32,
    data: Vec<u8>,
}
#[repr(C)]
struct Object {
    vtable: *const Vtable,
    refs: AtomicU32,
    entries: Arc<Vec<Entry>>,
}
pub struct Prepared {
    entries: Arc<Vec<Entry>>,
}
unsafe extern "system" fn query(
    this: *mut Object,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() || iid.is_null() {
        return 0x80004003u32 as i32;
    }
    *out = null_mut();
    // IUnknown and IDataObject.
    let id = &*iid;
    if (id.data1 == 0 || id.data1 == 0x10e)
        && id.data2 == 0
        && id.data3 == 0
        && id.data4 == [0xc0, 0, 0, 0, 0, 0, 0, 0x46]
    {
        *out = this as *mut c_void;
        add(this);
        0
    } else {
        0x80004002u32 as i32
    }
}
unsafe extern "system" fn add(this: *mut Object) -> u32 {
    (*this).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn release(this: *mut Object) -> u32 {
    let n = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this));
    }
    n
}
unsafe fn find<'a>(this: *mut Object, fmt: *const FORMATETC) -> Option<&'a Entry> {
    if fmt.is_null() {
        return None;
    }
    let f = &*fmt;
    if f.tymed & 1 == 0 || f.dwAspect != 1 {
        return None;
    }
    (*this).entries.iter().find(|e| {
        e.format == f.cfFormat && (e.index == f.lindex || (e.index == 0 && f.lindex == -1))
    })
}
unsafe extern "system" fn get(
    this: *mut Object,
    fmt: *const FORMATETC,
    out: *mut STGMEDIUM,
) -> HRESULT {
    if out.is_null() {
        return 0x80004003u32 as i32;
    }
    *out = std::mem::zeroed();
    let Some(e) = find(this, fmt) else {
        return FORMAT_ERROR;
    };
    let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, e.data.len());
    if h.is_null() {
        return 0x8007000eu32 as i32;
    }
    let p = GlobalLock(h);
    if p.is_null() {
        GlobalFree(h);
        return 0x8007000eu32 as i32;
    }
    std::ptr::copy_nonoverlapping(e.data.as_ptr(), p as *mut u8, e.data.len());
    GlobalUnlock(h);
    (*out).tymed = 1;
    (*out).u.hGlobal = h;
    0
}
unsafe extern "system" fn get_here(
    _: *mut Object,
    _: *const FORMATETC,
    _: *mut STGMEDIUM,
) -> HRESULT {
    UNSUPPORTED
}
unsafe extern "system" fn query_get(this: *mut Object, fmt: *const FORMATETC) -> HRESULT {
    if find(this, fmt).is_some() {
        0
    } else {
        FORMAT_ERROR
    }
}
unsafe extern "system" fn canonical(
    _: *mut Object,
    _: *const FORMATETC,
    out: *mut FORMATETC,
) -> HRESULT {
    if !out.is_null() {
        (*out).ptd = null_mut();
    }
    UNSUPPORTED
}
unsafe extern "system" fn set(
    _: *mut Object,
    _: *const FORMATETC,
    _: *const STGMEDIUM,
    _: i32,
) -> HRESULT {
    UNSUPPORTED
}
unsafe extern "system" fn enumeration(
    this: *mut Object,
    direction: u32,
    out: *mut *mut c_void,
) -> HRESULT {
    if direction != 1 {
        return UNSUPPORTED;
    }
    let formats: Vec<_> = (*this)
        .entries
        .iter()
        .map(|e| FORMATETC {
            cfFormat: e.format,
            ptd: null_mut(),
            dwAspect: 1,
            lindex: e.index,
            tymed: 1,
        })
        .collect();
    SHCreateStdEnumFmtEtc(formats.len() as u32, formats.as_ptr(), out)
}
unsafe extern "system" fn advise(
    _: *mut Object,
    _: *const FORMATETC,
    _: u32,
    _: *mut c_void,
    _: *mut u32,
) -> HRESULT {
    ADVISE_ERROR
}
unsafe extern "system" fn unadvise(_: *mut Object, _: u32) -> HRESULT {
    ADVISE_ERROR
}
unsafe extern "system" fn enum_advise(_: *mut Object, _: *mut *mut c_void) -> HRESULT {
    ADVISE_ERROR
}
static VTABLE: Vtable = Vtable {
    query,
    add,
    release,
    get,
    get_here,
    query_get,
    canonical,
    set,
    enumeration,
    advise,
    unadvise,
    enum_advise,
};
fn utf16bytes(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}
fn format(name: &str) -> u16 {
    unsafe {
        windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW(
            super::wide(name).as_ptr(),
        ) as u16
    }
}
// A file attachment alone cannot be pasted by bitmap-only applications such
// as painting programs. Supply native DIB data alongside the compressed file.
// Keep file formats first so file-aware consumers retain the real attachment.
fn bitmap_entries(data: &[u8]) -> Option<Vec<Entry>> {
    let image = image::load_from_memory(data).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    let width_i32 = i32::try_from(width).ok()?;
    let height_i32 = i32::try_from(height).ok()?;
    let transparent = image.pixels().any(|p| p[3] != 255);
    let bits = if transparent { 32u16 } else { 24u16 };
    let stride = (width as usize)
        .checked_mul(bits as usize)?
        .checked_add(31)?
        / 32
        * 4;
    let size = stride.checked_mul(height as usize)?;
    let rgba_size = (width as usize)
        .checked_mul(4)?
        .checked_mul(height as usize)?;
    if size > 512 * 1024 * 1024 || rgba_size > 512 * 1024 * 1024 {
        return None;
    }
    let mut dib = vec![0; 40usize.checked_add(size)?];
    dib[..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&width_i32.to_le_bytes());
    dib[8..12].copy_from_slice(&height_i32.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&bits.to_le_bytes());
    dib[20..24].copy_from_slice(&(size as u32).to_le_bytes());
    let mut v5 = vec![0; 124usize.checked_add(rgba_size)?];
    v5[..40].copy_from_slice(&dib[..40]);
    v5[..4].copy_from_slice(&124u32.to_le_bytes());
    v5[14..16].copy_from_slice(&32u16.to_le_bytes());
    v5[16..20].copy_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
    v5[20..24].copy_from_slice(&(rgba_size as u32).to_le_bytes());
    for (offset, mask) in [
        (40, 0x00ff0000u32),
        (44, 0x0000ff00),
        (48, 0x000000ff),
        (52, 0xff000000),
    ] {
        v5[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
    }
    v5[56..60].copy_from_slice(&0x73524742u32.to_le_bytes()); // LCS_sRGB
    v5[108..112].copy_from_slice(&4u32.to_le_bytes()); // LCS_GM_IMAGES
    for y in 0..height as usize {
        for x in 0..width as usize {
            let p = image.get_pixel(x as u32, (height as usize - 1 - y) as u32);
            let bgra = [p[2], p[1], p[0], p[3]];
            let offset = 40 + y * stride + x * (bits as usize / 8);
            dib[offset..offset + bits as usize / 8].copy_from_slice(&bgra[..bits as usize / 8]);
            let offset = 124 + (y * width as usize + x) * 4;
            v5[offset..offset + 4].copy_from_slice(&bgra);
        }
    }
    Some(vec![
        Entry {
            format: 17,
            index: -1,
            data: v5,
        },
        Entry {
            format: 8,
            index: -1,
            data: dib,
        },
    ])
}
pub fn prepare(paths: &[PathBuf]) -> std::io::Result<Prepared> {
    let mut entries = Vec::new();
    let mut native_bitmap = None;
    let mut drop = vec![0; 20];
    drop[..4].copy_from_slice(&20u32.to_le_bytes());
    drop[16..20].copy_from_slice(&1u32.to_le_bytes());
    let mut descriptors = vec![0; 4 + 592 * paths.len()];
    descriptors[..4].copy_from_slice(&(paths.len() as u32).to_le_bytes());
    let contents = format("FileContents");
    for (i, path) in paths.iter().enumerate() {
        let data = std::fs::read(path)?;
        if data.is_empty() {
            return Err(std::io::Error::other("empty clipboard file"));
        }
        drop.extend(utf16bytes(&path.to_string_lossy()));
        let d = &mut descriptors[4 + 592 * i..4 + 592 * (i + 1)];
        d[..4].copy_from_slice(&0x80000044u32.to_le_bytes()); // FD_UNICODE | FD_FILESIZE | FD_ATTRIBUTES
        d[36..40].copy_from_slice(&128u32.to_le_bytes());
        let n = data.len() as u64;
        d[64..68].copy_from_slice(&((n >> 32) as u32).to_le_bytes());
        d[68..72].copy_from_slice(&(n as u32).to_le_bytes());
        let name = utf16bytes(&path.file_name().unwrap_or_default().to_string_lossy());
        let len = name.len().min(518);
        d[72..72 + len].copy_from_slice(&name[..len]);
        if paths.len() == 1 {
            native_bitmap = bitmap_entries(&data);
            entries.push(Entry {
                format: format("FileNameW"),
                index: -1,
                data: utf16bytes(&path.to_string_lossy()),
            });
            // Native image readers can also consume the encoded bytes directly.
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if ext == "png" {
                entries.push(Entry {
                    format: format("PNG"),
                    index: -1,
                    data: data.clone(),
                });
            }
        }
        entries.push(Entry {
            format: contents,
            index: i as i32,
            data,
        });
    }
    drop.extend([0, 0]);
    entries.insert(
        0,
        Entry {
            format: 15,
            index: -1,
            data: drop,
        },
    );
    entries.push(Entry {
        format: format("FileGroupDescriptorW"),
        index: -1,
        data: descriptors,
    });
    entries.push(Entry {
        format: format("Preferred DropEffect"),
        index: -1,
        data: 1u32.to_le_bytes().to_vec(),
    });
    if let Some(bitmap) = native_bitmap {
        entries.extend(bitmap);
    }
    Ok(Prepared {
        entries: Arc::new(entries),
    })
}
pub unsafe fn publish(prepared: &Prepared) -> Result<(), HRESULT> {
    let object = Box::into_raw(Box::new(Object {
        vtable: &VTABLE,
        refs: AtomicU32::new(1),
        entries: prepared.entries.clone(),
    }));
    let hr = OleSetClipboard(object as *mut c_void);
    release(object);
    if hr >= 0 {
        Ok(())
    } else {
        Err(hr)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converted_webp_provides_png_attachment_and_matching_alpha_bitmap() {
        let original = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            3,
            2,
            image::Rgba([50, 100, 150, 127]),
        ));
        let mut webp = std::io::Cursor::new(Vec::new());
        original
            .write_to(&mut webp, image::ImageFormat::WebP)
            .unwrap();
        let decoded = image::load_from_memory(webp.get_ref()).unwrap();
        let converted = crate::imaging::encoded_as(
            &decoded,
            webp.get_ref().len(),
            95,
            100,
            crate::imaging::Compression::Jpg,
            image::ImageFormat::WebP,
            true,
            true,
        )
        .unwrap();
        assert_eq!(converted.extension, "png");
        let dir =
            std::env::temp_dir().join(format!("avenger-webp-ole-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("converted.png");
        std::fs::write(&path, &converted.bytes).unwrap();
        let prepared = prepare(&[path.clone()]).unwrap();
        assert_eq!(prepared.entries[0].format, 15);
        for name in ["FileContents", "PNG"] {
            let entry = prepared
                .entries
                .iter()
                .find(|e| e.format == format(name))
                .unwrap();
            assert_eq!(entry.data, converted.bytes);
        }
        let descriptor = prepared
            .entries
            .iter()
            .find(|e| e.format == format("FileGroupDescriptorW"))
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(descriptor.data[72..76].try_into().unwrap()) as usize,
            converted.bytes.len()
        );
        let filename: Vec<u16> = descriptor.data[76..]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|&c| c != 0)
            .collect();
        assert_eq!(String::from_utf16(&filename).unwrap(), "converted.png");
        let v5 = prepared.entries.iter().find(|e| e.format == 17).unwrap();
        assert_eq!(
            crate::imaging::dib_image(&v5.data).unwrap().to_rgba8(),
            decoded.to_rgba8()
        );
        assert!(prepared.entries.iter().any(|e| e.format == 8));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn jpeg_attachment_also_offers_native_bitmaps_with_correct_rows_and_colors() {
        use image::ImageEncoder;
        // An odd width exercises DWORD row padding in the legacy 24-bit DIB.
        let image = image::RgbImage::from_fn(7, 5, |x, y| {
            image::Rgb([(x * 30) as u8, (y * 40) as u8, 90])
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(image.as_raw(), 7, 5, image::ExtendedColorType::Rgb8)
            .unwrap();
        let expected = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        let dir = std::env::temp_dir().join(format!("avenger-bitmap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("image.jpg");
        std::fs::write(&path, &jpeg).unwrap();
        let prepared = prepare(&[path.clone()]).unwrap();
        assert_eq!(prepared.entries[0].format, 15); // File attachment stays first.
        let contents = prepared
            .entries
            .iter()
            .find(|e| e.format == format("FileContents"))
            .unwrap();
        assert_eq!(contents.data, jpeg);
        for format in [8, 17] {
            let entry = prepared
                .entries
                .iter()
                .find(|e| e.format == format)
                .unwrap();
            let decoded = super::super::imaging::dib_image(&entry.data).unwrap();
            assert_eq!(decoded.to_rgb8(), expected);
            unsafe {
                let object = Box::into_raw(Box::new(Object {
                    vtable: &VTABLE,
                    refs: AtomicU32::new(1),
                    entries: prepared.entries.clone(),
                }));
                let fmt = FORMATETC {
                    cfFormat: format,
                    ptd: null_mut(),
                    dwAspect: 1,
                    lindex: -1,
                    tymed: 1,
                };
                assert_eq!(query_get(object, &fmt), 0);
                let mut medium: STGMEDIUM = std::mem::zeroed();
                assert_eq!(get(object, &fmt, &mut medium), 0);
                assert_eq!(medium.tymed, 1);
                let memory = GlobalLock(medium.u.hGlobal);
                assert!(!memory.is_null());
                assert_eq!(
                    std::slice::from_raw_parts(memory as *const u8, entry.data.len()),
                    entry.data
                );
                GlobalUnlock(medium.u.hGlobal);
                ReleaseStgMedium(&mut medium);
                release(object);
            }
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn alpha_bitmap_preserves_every_transparent_pixel() {
        let expected = image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([(x * 90) as u8, (y * 120) as u8, 70, (x * 70 + y * 30) as u8])
        });
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(expected.clone())
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let entries = bitmap_entries(png.get_ref()).unwrap();
        let v5 = entries.iter().find(|e| e.format == 17).unwrap();
        assert_eq!(
            super::super::imaging::dib_image(&v5.data)
                .unwrap()
                .to_rgba8(),
            expected
        );
        let dib = entries.iter().find(|e| e.format == 8).unwrap();
        assert_eq!(&dib.data[14..16], &32u16.to_le_bytes());
        assert_eq!(dib.data[43], expected.get_pixel(0, 1)[3]);
        assert!(bitmap_entries(b"plain text").is_none());
    }
    #[test]
    fn every_file_has_matching_name_size_and_indexed_content() {
        let dir = std::env::temp_dir().join(format!("avenger-ole-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = vec![dir.join("first.jpg"), dir.join("second.png")];
        std::fs::write(&paths[0], [1, 2, 3]).unwrap();
        std::fs::write(&paths[1], [4, 5, 6, 7, 8]).unwrap();
        let p = prepare(&paths).unwrap();
        assert!(p
            .entries
            .iter()
            .all(|entry| entry.format != 8 && entry.format != 17));
        let desc = p
            .entries
            .iter()
            .find(|e| e.format == format("FileGroupDescriptorW"))
            .unwrap();
        assert_eq!(u32::from_le_bytes(desc.data[..4].try_into().unwrap()), 2);
        for (i, n) in [3u32, 5].iter().enumerate() {
            let d = &desc.data[4 + 592 * i..];
            assert_eq!(u32::from_le_bytes(d[68..72].try_into().unwrap()), *n);
            let e = p
                .entries
                .iter()
                .find(|e| e.format == format("FileContents") && e.index == i as i32)
                .unwrap();
            assert_eq!(e.data.len(), *n as usize);
        }
        std::fs::remove_file(&paths[0]).unwrap();
        std::fs::remove_file(&paths[1]).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
