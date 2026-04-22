mod calibrate;
mod config;
mod filter;
mod tray;

use anyhow::Result;

fn main() -> Result<()> {
    let cfg = config::Config::load()?;
    let filter = filter::Filter::new(cfg);
    let _ = filter;
    // TODO: install WH_KEYBOARD_LL hook, run GetMessage loop, init tray
    Ok(())
}
