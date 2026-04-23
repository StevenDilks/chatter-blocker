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

    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    pub fn on_key_down(&mut self, vk: u32, time_ms: u32) -> Decision {
        let threshold = self.cfg.threshold(vk);
        let state = self.state.entry(vk).or_default();

        // Auto-repeat intervals don't fall below typical thresholds (~25–30 ms) in
        // practice, so a gap inside the window means chatter in either branch.
        let suppress = if state.is_held {
            let gap = time_ms.saturating_sub(state.last_down_ms);
            if gap < threshold {
                true
            } else {
                state.last_down_ms = time_ms;
                false
            }
        } else {
            let gap = time_ms.saturating_sub(state.last_up_ms);
            if state.last_up_ms != 0 && gap < threshold {
                true
            } else {
                state.last_down_ms = time_ms;
                state.is_held = true;
                false
            }
        };

        if suppress {
            *self.suppressed_count.entry(vk).or_insert(0) += 1;
            Decision::Suppress
        } else {
            Decision::Pass
        }
    }

    pub fn on_key_up(&mut self, vk: u32, time_ms: u32) -> Decision {
        // UPs always pass. Suppressing them would cause stuck modifiers (Ctrl,
        // Shift, Alt) at any non-trivial threshold since held time is usually
        // shorter than the debounce window. A mid-hold bounce still briefly
        // looks like a release to the app, but that's cheaper than stuck keys.
        let state = self.state.entry(vk).or_default();
        state.last_up_ms = time_ms;
        state.is_held = false;
        Decision::Pass
    }

    pub fn suppressed(&self, vk: u32) -> u64 {
        *self.suppressed_count.get(&vk).unwrap_or(&0)
    }

    pub fn total_suppressed(&self) -> u64 {
        self.suppressed_count.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(threshold_ms: u32) -> Filter {
        let cfg = Config {
            default_threshold_ms: threshold_ms,
            ..Config::default()
        };
        Filter::new(cfg)
    }

    const A: u32 = 0x41;
    const B: u32 = 0x42;

    #[test]
    fn single_press_passes() {
        let mut f = filter(30);
        assert!(matches!(f.on_key_down(A, 100), Decision::Pass));
        assert!(matches!(f.on_key_up(A, 150), Decision::Pass));
        assert_eq!(f.total_suppressed(), 0);
    }

    #[test]
    fn chatter_down_after_real_tap_suppressed() {
        let mut f = filter(30);
        f.on_key_down(A, 100);
        f.on_key_up(A, 150);
        assert!(matches!(f.on_key_down(A, 160), Decision::Suppress));
        assert_eq!(f.suppressed(A), 1);
    }

    #[test]
    fn auto_repeat_passes() {
        let mut f = filter(25);
        f.on_key_down(A, 0);
        assert!(matches!(f.on_key_down(A, 500), Decision::Pass));
        assert!(matches!(f.on_key_down(A, 533), Decision::Pass));
        assert!(matches!(f.on_key_down(A, 566), Decision::Pass));
        assert_eq!(f.total_suppressed(), 0);
    }

    #[test]
    fn bounce_back_down_after_spurious_up_suppressed() {
        // Mid-hold bounce: the spurious UP leaks through (UPs always pass),
        // but the bounce-back DOWN that would produce a duplicate character
        // is blocked.
        let mut f = filter(30);
        f.on_key_down(A, 100);
        assert!(matches!(f.on_key_up(A, 105), Decision::Pass));
        assert!(matches!(f.on_key_down(A, 110), Decision::Suppress));
        assert!(matches!(f.on_key_up(A, 600), Decision::Pass));
        assert_eq!(f.suppressed(A), 1);
    }

    #[test]
    fn legitimate_fast_tap_passes() {
        let mut f = filter(30);
        f.on_key_down(A, 0);
        f.on_key_up(A, 40);
        assert!(matches!(f.on_key_down(A, 80), Decision::Pass));
        assert!(matches!(f.on_key_up(A, 120), Decision::Pass));
        assert_eq!(f.total_suppressed(), 0);
    }

    #[test]
    fn different_keys_do_not_interfere() {
        let mut f = filter(30);
        f.on_key_down(A, 0);
        f.on_key_up(A, 50);
        // Would look like chatter for A (5 ms after A's release), but B has clean state.
        assert!(matches!(f.on_key_down(B, 55), Decision::Pass));
        assert!(matches!(f.on_key_up(B, 100), Decision::Pass));
    }

    #[test]
    fn per_key_threshold_overrides_default() {
        let mut cfg = Config {
            default_threshold_ms: 30,
            ..Config::default()
        };
        cfg.per_key_threshold_ms.insert(A, 0);
        let mut f = Filter::new(cfg);
        f.on_key_down(A, 0);
        f.on_key_up(A, 3);
        assert!(matches!(f.on_key_down(A, 5), Decision::Pass));
        assert_eq!(f.total_suppressed(), 0);
    }
}
