use std::{
    io,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::*,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Clipboard Avenger";
const LEGACY_VALUE_NAME: &str = "Desktop Image Avenger";

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn executable_command(executable: &Path) -> io::Result<Vec<u16>> {
    let path: Vec<u16> = executable.as_os_str().encode_wide().collect();
    if path.is_empty() || path.iter().any(|&c| c == 0 || c == b'"' as u16) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The executable path cannot be used as a Windows startup command.",
        ));
    }
    // Run values support commands of at most 260 characters, including quotes.
    if path.len() + 2 > 260 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Move the executable to a shorter path before enabling Start with Windows.",
        ));
    }
    Ok(std::iter::once(b'"' as u16)
        .chain(path)
        .chain([b'"' as u16, 0])
        .collect())
}

fn checked(status: u32) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

fn missing(status: u32) -> bool {
    status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND
}

fn remove_values(key: HKEY, names: &[&str]) -> io::Result<()> {
    let mut first_error = None;
    for name in names {
        let status = unsafe { RegDeleteValueW(key, wide(name).as_ptr()) };
        if status != ERROR_SUCCESS && !missing(status) && first_error.is_none() {
            first_error = Some(io::Error::from_raw_os_error(status as i32));
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Register this executable for the current user's Windows sign-in, or remove
/// only this application's registration. No administrator privileges are needed.
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let key_name = wide(RUN_KEY);
    let value_name = wide(VALUE_NAME);
    // Resolve/validate before creating or changing any registry state.
    let command = if enabled {
        Some(executable_command(&std::env::current_exe()?)?)
    } else {
        None
    };
    unsafe {
        let mut handle = null_mut();
        if let Some(command) = command {
            checked(RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key_name.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                null(),
                &mut handle,
                null_mut(),
            ))?;
            let key = Key(handle);
            let byte_len = (command.len() * std::mem::size_of::<u16>()) as u32;
            let mut existing = vec![0u16; command.len()];
            let mut existing_len = byte_len;
            let mut existing_type = 0;
            let status = RegQueryValueExW(
                key.0,
                value_name.as_ptr(),
                null_mut(),
                &mut existing_type,
                existing.as_mut_ptr().cast(),
                &mut existing_len,
            );
            // Avoid rewriting the Run key when restoring an unchanged setting.
            let unchanged = status == ERROR_SUCCESS
                && existing_type == REG_SZ
                && existing_len == byte_len
                && existing == command;
            if !unchanged {
                checked(RegSetValueExW(
                    key.0,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    command.as_ptr().cast(),
                    byte_len,
                ))?;
            }
            // Only remove the former registration once the new one is valid.
            // Also clean it up when the new entry already matched this path.
            remove_values(key.0, &[LEGACY_VALUE_NAME])
        } else {
            let status = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key_name.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut handle,
            );
            if missing(status) {
                return Ok(());
            }
            checked(status)?;
            let key = Key(handle);
            remove_values(key.0, &[VALUE_NAME, LEGACY_VALUE_NAME])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_spaces_and_preserves_unicode() {
        let path = Path::new(r"C:\Users\Zoë 画像\Clipboard Avenger.exe");
        let command = executable_command(path).unwrap();
        assert_eq!(command.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&command[..command.len() - 1]).unwrap(),
            "\"C:\\Users\\Zoë 画像\\Clipboard Avenger.exe\""
        );
    }

    #[test]
    fn startup_command_rejects_unusable_paths() {
        assert!(executable_command(Path::new("bad\"path.exe")).is_err());
        assert!(executable_command(Path::new("bad\0path.exe")).is_err());
        assert!(executable_command(Path::new("")).is_err());
        assert!(executable_command(Path::new(&"a".repeat(259))).is_err());
        assert!(executable_command(Path::new(&"a".repeat(258))).is_ok());
    }
}
