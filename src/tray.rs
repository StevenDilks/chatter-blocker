use anyhow::Result;

pub struct Tray;

impl Tray {
    pub fn new() -> Result<Self> {
        todo!("Shell_NotifyIcon + right-click menu (enable/disable, open config, stats)")
    }

    pub fn set_enabled(&self, _enabled: bool) {
        todo!()
    }

    pub fn update_stats(&self, _total_suppressed: u64) {
        todo!()
    }
}
