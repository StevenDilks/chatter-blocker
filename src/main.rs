#![windows_subsystem = "console"]

mod calibrate;
mod config;
mod filter;
mod tray;

use anyhow::{anyhow, Result};
use filter::{Decision, Filter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

static FILTER: OnceLock<Mutex<Filter>> = OnceLock::new();
static DEBUG_LOG: AtomicBool = AtomicBool::new(false);

fn filter() -> &'static Mutex<Filter> {
    FILTER.get().expect("filter not initialized before hook fired")
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

    // Skip events injected by other processes (AutoHotkey, on-screen keyboards, SendInput).
    if info.flags.0 & LLKHF_INJECTED.0 != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let vk = info.vkCode;
    let time = info.time;

    let decision = {
        let mut f = filter().lock().unwrap();
        match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => f.on_key_down(vk, time),
            WM_KEYUP | WM_SYSKEYUP => f.on_key_up(vk, time),
            _ => Decision::Pass,
        }
    };

    match decision {
        Decision::Suppress => {
            if DEBUG_LOG.load(Ordering::Relaxed) {
                let kind = match wparam.0 as u32 {
                    WM_KEYDOWN | WM_SYSKEYDOWN => "down",
                    WM_KEYUP | WM_SYSKEYUP => "up",
                    _ => "?",
                };
                println!("suppress {kind} vk=0x{vk:02X} t={time}");
            }
            LRESULT(1)
        }
        Decision::Pass => unsafe { CallNextHookEx(None, code, wparam, lparam) },
    }
}

fn main() -> Result<()> {
    DEBUG_LOG.store(std::env::var_os("CHATTER_LOG").is_some(), Ordering::Relaxed);

    let cfg = config::Config::load()?;
    FILTER
        .set(Mutex::new(Filter::new(cfg)))
        .map_err(|_| anyhow!("filter already initialized"))?;

    unsafe {
        let hmod = GetModuleHandleW(None)?;
        let hook: HHOOK =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(hmod.into()), 0)?;

        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWindowsHookEx(hook);
    }

    Ok(())
}
