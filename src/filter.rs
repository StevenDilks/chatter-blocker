use crate::config::Config;
use std::collections::HashMap;

#[derive(Default, Clone, Copy)]
struct KeyState {
    last_down_ms: u32,
    last_up_ms: u32,
    is_held: bool,
}

pub enum Decision {
    Pass,
    Suppress,
}

pub struct Filter {
    cfg: Config,
    state: HashMap<u32, KeyState>,
    suppressed_count: HashMap<u32, u64>,
}

impl Filter {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            state: HashMap::new(),
            suppressed_count: HashMap::new(),
        }
    }

    pub fn on_key_down(&mut self, _vk: u32, _time_ms: u32) -> Decision {
        todo!("if !is_held && (time_ms - last_up_ms) < threshold[vk] => Suppress")
    }

    pub fn on_key_up(&mut self, _vk: u32, _time_ms: u32) -> Decision {
        todo!("if (time_ms - last_down_ms) < threshold[vk] => Suppress")
    }

    pub fn suppressed(&self, vk: u32) -> u64 {
        *self.suppressed_count.get(&vk).unwrap_or(&0)
    }

    pub fn total_suppressed(&self) -> u64 {
        self.suppressed_count.values().sum()
    }
}
