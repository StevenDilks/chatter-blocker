use anyhow::{anyhow, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE_NAME: PCWSTR = w!("ChatterBlocker");

pub fn is_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ, &mut hkey).is_err() {
            return false;
        }
        let exists = RegQueryValueExW(hkey, VALUE_NAME, None, None, None, None).is_ok();
        let _ = RegCloseKey(hkey);
        exists
    }
}

pub fn set(enable: bool) -> Result<()> {
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_WRITE, &mut hkey)
            .ok()
            .map_err(|e| anyhow!("open Run key: {e}"))?;
        let result = if enable {
            let path = current_exe_quoted()?;
            let bytes: &[u8] = std::slice::from_raw_parts(
                path.as_ptr() as *const u8,
                path.len() * std::mem::size_of::<u16>(),
            );
            RegSetValueExW(hkey, VALUE_NAME, None, REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| anyhow!("write value: {e}"))
        } else {
            RegDeleteValueW(hkey, VALUE_NAME)
                .ok()
                .map_err(|e| anyhow!("delete value: {e}"))
        };
        let _ = RegCloseKey(hkey);
        result
    }
}

/// Returns the full path to the running exe as a null-terminated UTF-16 buffer,
/// wrapped in double quotes so paths containing spaces round-trip safely.
fn current_exe_quoted() -> Result<Vec<u16>> {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(None, &mut buf) };
    if n == 0 {
        return Err(anyhow!("GetModuleFileNameW failed"));
    }
    let path = &buf[..n as usize];
    let mut out = Vec::with_capacity(path.len() + 3);
    out.push(b'"' as u16);
    out.extend_from_slice(path);
    out.push(b'"' as u16);
    out.push(0);
    Ok(out)
}
