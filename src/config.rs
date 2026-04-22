use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub enabled: bool,
    pub default_threshold_ms: u32,
    pub per_key_threshold_ms: HashMap<u32, u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            default_threshold_ms: 30,
            per_key_threshold_ms: HashMap::new(),
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let appdata = std::env::var("APPDATA")?;
        Ok(PathBuf::from(appdata)
            .join("ChatterBlocker")
            .join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn threshold(&self, vk: u32) -> u32 {
        *self
            .per_key_threshold_ms
            .get(&vk)
            .unwrap_or(&self.default_threshold_ms)
    }
}
