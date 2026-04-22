use std::collections::HashMap;

pub struct Calibrator {
    intervals_ms: HashMap<u32, Vec<u32>>,
}

impl Calibrator {
    pub fn new() -> Self {
        Self {
            intervals_ms: HashMap::new(),
        }
    }

    pub fn record(&mut self, _vk: u32, _interval_ms: u32) {
        todo!("push interval onto per-vk list for later histogram analysis")
    }

    pub fn suggest_threshold(&self, _vk: u32) -> Option<u32> {
        todo!("pick a threshold above the observed chatter cluster (~20ms) and below real presses")
    }
}

impl Default for Calibrator {
    fn default() -> Self {
        Self::new()
    }
}
