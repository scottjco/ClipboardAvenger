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
pub fn prepare(paths: &[PathBuf]) -> std::io::Result<Prepared> {
    let mut entries = Vec::new();
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
    fn every_file_has_matching_name_size_and_indexed_content() {
        let dir = std::env::temp_dir().join(format!("avenger-ole-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = vec![dir.join("first.jpg"), dir.join("second.png")];
        std::fs::write(&paths[0], [1, 2, 3]).unwrap();
        std::fs::write(&paths[1], [4, 5, 6, 7, 8]).unwrap();
        let p = prepare(&paths).unwrap();
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
