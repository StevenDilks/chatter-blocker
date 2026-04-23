#![windows_subsystem = "windows"]

mod autostart;
mod calibrate;
mod config;
mod filter;
mod tray;

use anyhow::{anyhow, Result};
use calibrate::Calibrator;
use filter::{Decision, Filter};
use notify::{RecursiveMode, Watcher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
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

pub fn total_suppressed() -> u64 {
    // try_lock to avoid stalling the UI thread if the hook is mid-update.
    FILTER
        .get()
        .and_then(|m| m.try_lock().ok())
        .map(|f| f.total_suppressed())
        .unwrap_or(0)
}

fn spawn_config_watcher() {
    std::thread::spawn(|| {
        let Ok(path) = config::Config::path() else {
            return;
        };
        let Some(dir) = path.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let Ok(mut watcher) = notify::recommended_watcher(tx) else {
            return;
        };
        // Watching the parent dir (not the file) survives editor save-via-rename.
        if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
            return;
        }
        while let Ok(event) = rx.recv() {
            let Ok(event) = event else { continue };
            if !event.paths.iter().any(|p| p == &path) {
                continue;
            }
            // Editors emit several events per save; coalesce them.
            std::thread::sleep(Duration::from_millis(100));
            while rx.try_recv().is_ok() {}
            match config::Config::load() {
                Ok(cfg) => {
                    ENABLED.store(cfg.enabled, Ordering::Relaxed);
                    filter().lock().unwrap().set_config(cfg);
                    eprintln!("[config] reloaded");
                }
                Err(e) => eprintln!("[config] reload failed: {e}"),
            }
        }
    });
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
                eprintln!("suppress {kind} vk=0x{vk:02X} t={time}");
            }
            LRESULT(1)
        }
        Decision::Pass => unsafe { CallNextHookEx(None, code, wparam, lparam) },
    }
}

fn main() -> Result<()> {
    // Attach to the parent's console if launched from a terminal so eprintln!
    // is visible; silently fails (and no console appears) for Explorer launches.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    // Single-instance guard: double-clicking the exe from Explorer while the
    // tray copy is already running would otherwise stack multiple icons and
    // register duplicate LL hooks. The handle is intentionally left open for
    // the life of the process; ExitProcess releases the name.
    unsafe {
        let _mutex = CreateMutexW(None, false, w!("Global\\ChatterBlockerSingleton"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
    }

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
        eprintln!("[calibrate] mode active — filter bypassed, recording DOWN gaps");
        eprintln!("[calibrate] report prints every 5s; Ctrl+C when done");
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(5));
            let snapshot = CALIBRATOR.get().unwrap().lock().unwrap().snapshot();
            eprint!("{}", calibrate::format_report(&snapshot));
        });
    } else {
        eprintln!("[chatter-blocker] running; filter threshold from config; Ctrl+C to quit");
        spawn_config_watcher();
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
