# chatter-blocker

A Windows tray utility that suppresses **keyboard chatter** — the duplicate key events a worn or bouncy mechanical switch emits when physically pressed once.

Installs a low-level keyboard hook, timestamps events per virtual-key, and swallows any event arriving inside a configurable debounce window. Filtering happens before the event reaches any application.

## Install

### Prerequisites

**1. MSVC build tools.** Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and, in the installer, check **"Desktop development with C++"**. This pulls in the MSVC compiler, Windows SDK, and linker that Rust's `x86_64-pc-windows-msvc` target requires.

**2. Rust.** Install via [rustup](https://rustup.rs/) — download and run `rustup-init.exe`. Accept the default `stable-x86_64-pc-windows-msvc` toolchain. Open a new terminal after install so `cargo` is on `PATH`.

Verify:

```
rustc --version
cargo --version
```

### Build

```
git clone https://github.com/StevenDilks/chatter-blocker
cd chatter-blocker
cargo build --release
```

Run `target/release/chatter-blocker.exe` — a tray icon (teal "B" keycap) appears.

## Tray menu

Right-click the tray icon:

- **Suppressed: N** — running count of blocked events (refreshes in the tooltip every 2s).
- **Disable / Enable** — toggle filtering without quitting.
- **Open config folder** — jumps to `%APPDATA%\ChatterBlocker`.
- **Start with Windows** — adds/removes a `HKCU\...\Run` registry entry.
- **Start calibration** / **Stop calibration** — record inter-press gaps to identify chattering keys (see [Calibration](#calibration)).
- **Quit**.

## Configuration

`%APPDATA%\ChatterBlocker\config.toml`:

```toml
enabled = true
default_threshold_ms = 30

[per_key_threshold_ms]
# virtual-key code = threshold
66 = 30   # 'B'
```

Keys are [Windows virtual-key codes](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes) in decimal. `0x41`–`0x5A` are `'A'`–`'Z'`.

**The config is hot-reloaded** — edit and save, and the running instance picks up changes without a restart (`[config] reloaded` in the console).

## Calibration

Calibration records the inter-press gap between consecutive presses of each key, buckets them into a histogram, and classifies each key as **chatter**, **suspicious**, **clean**, or **insufficient data**. Run it when you suspect a key is chattering — or just to confirm one isn't.

### From the tray

1. Right-click the tray icon → **Start calibration**. The filter is bypassed during the session.
2. Type normally, and press the suspect key(s) several times.
3. Right-click → either:
   - **Stop calibration (view report)** — writes `%APPDATA%\ChatterBlocker\calibration.txt` and opens it. The config is untouched.
   - **Stop calibration & apply to config** — does the same, *and* merges any chatter-classified suggestions into `[per_key_threshold_ms]` in your `config.toml`. The watcher hot-reloads the new thresholds immediately.

The apply path only writes thresholds for keys classified as **chatter** (≥3 gaps below 50 ms). Suspicious keys are never auto-applied — type more and rerun. Existing `per_key_threshold_ms` entries for keys not in this session are preserved.

### From a terminal

For streaming histograms to stderr, launch from a terminal with:

```
CHATTER_CALIBRATE=1 ./target/release/chatter-blocker.exe
```

A report prints every 5 seconds until Ctrl+C. Useful for live calibration sessions, though the tray path is the easier default.

### Reading the report

Chatter cluster usually lives below ~25 ms; legit same-key intervals start ~60 ms+. The suggested threshold lands just above the highest sub-50 ms gap for a chattering key. If a key shows as **suspicious** (1–2 sub-50 ms gaps), keep typing to gather more samples before trusting the signal.

## How it works

- `SetWindowsHookExW(WH_KEYBOARD_LL, …)` installs a process-wide hook on the input thread.
- Each `KEYDOWN` consults a per-vk state table of `{last_down_ms, last_up_ms, is_held}`; if the gap is under threshold, the event is suppressed.
- `KEYUP` events always pass through (suppressing them causes stuck modifiers).
- Auto-repeat is detected via the `is_held` flag and not treated as chatter.
- Events flagged `LLKHF_INJECTED` (AutoHotkey, on-screen keyboards) bypass the filter.

## Why not a Windows service?

Services run in session 0, which is isolated from the user's desktop — a keyboard hook there sees nothing useful. Use **Start with Windows** (registry Run key) to autostart on login instead.

## Layout

- `src/main.rs` — hook install, message loop, single-instance guard
- `src/filter.rs` — per-key state table and debounce logic
- `src/config.rs` — TOML load/save
- `src/calibrate.rs` — gap recording + histogram report
- `src/tray.rs` — tray icon, menu, custom icon
- `src/autostart.rs` — registry Run-key toggle
