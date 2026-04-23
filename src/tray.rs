use crate::config::Config;
use crate::ENABLED;
use anyhow::{anyhow, Result};
use std::sync::atomic::Ordering;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    GetCursorPos, PostMessageW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
    TrackPopupMenu, HICON, HMENU, HWND_MESSAGE, ICONINFO, MF_SEPARATOR, MF_STRING, SW_SHOW,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY,
    WM_NULL, WM_RBUTTONUP, WNDCLASSEXW,
};

const WM_TRAY: u32 = WM_APP + 1;
const IDM_TOGGLE: usize = 1001;
const IDM_OPEN_CONFIG: usize = 1002;
const IDM_QUIT: usize = 1003;
const TRAY_UID: u32 = 1;

pub fn install() -> Result<()> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let class_name = w!("ChatterBlockerTray");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(anyhow!("RegisterClassExW failed"));
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("ChatterBlocker"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinst.into()),
            None,
        )?;
        add_icon(hwnd)?;
    }
    Ok(())
}

unsafe fn add_icon(hwnd: HWND) -> Result<()> {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: unsafe { make_icon()? },
        ..Default::default()
    };
    write_tip(&mut nid.szTip, ENABLED.load(Ordering::Relaxed));
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
        return Err(anyhow!("Shell_NotifyIconW NIM_ADD failed"));
    }
    Ok(())
}

/// Build a 16x16 icon: a top-down view of a 'B' keycap.
/// '.' transparent (rounded corners), '#' border, 'k' keycap face, 'B' letter.
unsafe fn make_icon() -> Result<HICON> {
    const W: i32 = 16;
    const H: i32 = 16;
    const PX: usize = (W * H) as usize;

    const PATTERN: [&[u8; 16]; 16] = [
        b".##############.",
        b"#kkkkkkkkkkkkkk#",
        b"#kkkkkkkkkkkkkk#",
        b"#kkkkkkkkkkkkkk#",
        b"#kkkkBBBBkkkkkk#",
        b"#kkkkBkkkBkkkkk#",
        b"#kkkkBkkkBkkkkk#",
        b"#kkkkBBBBkkkkkk#",
        b"#kkkkBkkkBkkkkk#",
        b"#kkkkBkkkBkkkkk#",
        b"#kkkkBkkkBkkkkk#",
        b"#kkkkBBBBkkkkkk#",
        b"#kkkkkkkkkkkkkk#",
        b"#kkkkkkkkkkkkkk#",
        b"#kkkkkkkkkkkkkk#",
        b".##############.",
    ];

    let mut color = [0u8; PX * 4];
    for y in 0..H as usize {
        for x in 0..W as usize {
            let i = y * W as usize + x;
            let (r, g, b, a) = match PATTERN[y][x] {
                b'.' => (0, 0, 0, 0),
                b'#' | b'B' => (40, 40, 40, 255),
                b'k' => (235, 235, 235, 255),
                _ => (255, 0, 255, 255),
            };
            color[i * 4] = b;
            color[i * 4 + 1] = g;
            color[i * 4 + 2] = r;
            color[i * 4 + 3] = a;
        }
    }
    let mask = [0u8; PX / 8];

    let hbm_color = unsafe { CreateBitmap(W, H, 1, 32, Some(color.as_ptr() as *const _)) };
    let hbm_mask = unsafe { CreateBitmap(W, H, 1, 1, Some(mask.as_ptr() as *const _)) };
    let info = ICONINFO {
        fIcon: TRUE,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };
    let hicon = unsafe { CreateIconIndirect(&info)? };
    let _ = unsafe { DeleteObject(hbm_color.into()) };
    let _ = unsafe { DeleteObject(hbm_mask.into()) };
    Ok(hicon)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = l.0 as u32;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                unsafe { show_menu(hwnd) };
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (w.0 & 0xFFFF) as usize;
            match id {
                IDM_TOGGLE => {
                    let was = ENABLED.fetch_xor(true, Ordering::Relaxed);
                    unsafe { update_tooltip(hwnd, !was) };
                }
                IDM_OPEN_CONFIG => unsafe { open_config_folder() },
                IDM_QUIT => {
                    unsafe { remove_icon(hwnd) };
                    unsafe { PostQuitMessage(0) };
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { remove_icon(hwnd) };
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let hmenu: HMENU = match unsafe { CreatePopupMenu() } {
        Ok(h) => h,
        Err(_) => return,
    };
    let enabled = ENABLED.load(Ordering::Relaxed);
    let toggle_label = if enabled { w!("Disable") } else { w!("Enable") };
    let _ = unsafe { AppendMenuW(hmenu, MF_STRING, IDM_TOGGLE, toggle_label) };
    let _ = unsafe {
        AppendMenuW(
            hmenu,
            MF_STRING,
            IDM_OPEN_CONFIG,
            w!("Open config folder"),
        )
    };
    let _ = unsafe { AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null()) };
    let _ = unsafe { AppendMenuW(hmenu, MF_STRING, IDM_QUIT, w!("Quit")) };

    let mut pt = POINT::default();
    let _ = unsafe { GetCursorPos(&mut pt) };
    // SetForegroundWindow + trailing WM_NULL is the well-known fix for
    // tray menus that otherwise don't dismiss on outside clicks.
    let _ = unsafe { SetForegroundWindow(hwnd) };
    let _ = unsafe { TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None) };
    let _ = unsafe { PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)) };
    let _ = unsafe { DestroyMenu(hmenu) };
}

unsafe fn update_tooltip(hwnd: HWND, enabled: bool) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NIF_TIP,
        ..Default::default()
    };
    write_tip(&mut nid.szTip, enabled);
    let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

unsafe fn remove_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    };
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

unsafe fn open_config_folder() {
    let Ok(path) = Config::path() else { return };
    let Some(parent) = path.parent() else { return };
    let wide: Vec<u16> = parent
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let _ = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOW,
        )
    };
}

fn write_tip(buf: &mut [u16], enabled: bool) {
    let s = if enabled {
        "ChatterBlocker — on"
    } else {
        "ChatterBlocker — off"
    };
    let wide: Vec<u16> = s.encode_utf16().collect();
    let n = wide.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&wide[..n]);
    buf[n] = 0;
}
