#![windows_subsystem = "console"]

mod calibrate;
mod config;
mod filter;
mod tray;

use anyhow::{anyhow, Result};
use calibrate::Calibrator;
use filter::{Decision, Filter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

static FILTER: OnceLock<Mutex<Filter>> = OnceLock::new();
static CALIBRATOR: OnceLock<Mutex<Calibrator>> = OnceLock::new();
static DEBUG_LOG: AtomicBool = AtomicBool::new(false);
static CALIBRATE_MODE: AtomicBool = AtomicBool::new(false);
pub static ENABLED: AtomicBool = AtomicBool::new(true);

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

    if !ENABLED.load(Ordering::Relaxed) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let vk = info.vkCode;
    let time = info.time;
    let event_kind = wparam.0 as u32;
    let is_down = matches!(event_kind, WM_KEYDOWN | WM_SYSKEYDOWN);

    // Calibration mode: record DOWN gaps, bypass the filter entirely.
    if CALIBRATE_MODE.load(Ordering::Relaxed) {
        if is_down {
            if let Some(c) = CALIBRATOR.get() {
                c.lock().unwrap().record_down(vk, time);
            }
        }
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let decision = {
        let mut f = filter().lock().unwrap();
        match event_kind {
            WM_KEYDOWN | WM_SYSKEYDOWN => f.on_key_down(vk, time),
            WM_KEYUP | WM_SYSKEYUP => f.on_key_up(vk, time),
            _ => Decision::Pass,
        }
    };

    match decision {
        Decision::Suppress => {
            if DEBUG_LOG.load(Ordering::Relaxed) {
                let kind = if is_down { "down" } else { "up" };
                println!("suppress {kind} vk=0x{vk:02X} t={time}");
            }
            LRESULT(1)
        }
        Decision::Pass => unsafe { CallNextHookEx(None, code, wparam, lparam) },
    }
}

fn main() -> Result<()> {
    DEBUG_LOG.store(std::env::var_os("CHATTER_LOG").is_some(), Ordering::Relaxed);
    let calibrate_mode = std::env::var_os("CHATTER_CALIBRATE").is_some();
    CALIBRATE_MODE.store(calibrate_mode, Ordering::Relaxed);

    let cfg = config::Config::load()?;
    ENABLED.store(cfg.enabled, Ordering::Relaxed);
    FILTER
        .set(Mutex::new(Filter::new(cfg)))
        .map_err(|_| anyhow!("filter already initialized"))?;

    if calibrate_mode {
        CALIBRATOR
            .set(Mutex::new(Calibrator::new()))
            .map_err(|_| anyhow!("calibrator already initialized"))?;
        println!("[calibrate] mode active — filter bypassed, recording DOWN gaps");
        println!("[calibrate] report prints every 5s; Ctrl+C when done");
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(5));
            let snapshot = CALIBRATOR.get().unwrap().lock().unwrap().snapshot();
            print!("{}", calibrate::format_report(&snapshot));
        });
    }

    unsafe {
        let hmod = GetModuleHandleW(None)?;
        let hook: HHOOK =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(hmod.into()), 0)?;

        if !calibrate_mode {
            tray::install()?;
        }

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
