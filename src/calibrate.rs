use std::collections::HashMap;
use std::fmt::Write;

const BUCKET_MS: u32 = 5;
const MAX_MS: u32 = 200;
const N_BUCKETS: usize = (MAX_MS / BUCKET_MS) as usize;

/// Gaps at or above this aren't chatter — by this point you're into legit
/// fast-typing intervals. Keeps the detector from firing on a thin scatter
/// of 150+ ms gaps when there's no actual chatter cluster at all.
const CHATTER_CEILING_MS: u32 = 50;
/// Minimum sub-ceiling samples before we'll suggest a threshold.
const MIN_CHATTER_COUNT: usize = 3;

#[derive(Default)]
pub struct Calibrator {
    last_down_ms: HashMap<u32, u32>,
    gaps: HashMap<u32, Vec<u32>>,
}

impl Calibrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_down(&mut self, vk: u32, time_ms: u32) {
        if let Some(prev) = self.last_down_ms.insert(vk, time_ms) {
            let gap = time_ms.saturating_sub(prev);
            if gap > 0 && gap < 2000 {
                self.gaps.entry(vk).or_default().push(gap);
            }
        }
    }

    pub fn suggest_threshold(&self, vk: u32) -> Option<u32> {
        suggest_from_gaps(self.gaps.get(&vk)?)
    }

    pub fn snapshot(&self) -> HashMap<u32, Vec<u32>> {
        self.gaps.clone()
    }
}

/// Look for a chatter cluster — several gaps below CHATTER_CEILING_MS.
/// Returns the low edge of the next bucket above the highest sub-ceiling
/// gap. None when there aren't enough sub-ceiling samples to be confident.
pub fn suggest_from_gaps(gaps: &[u32]) -> Option<u32> {
    let max_low = gaps
        .iter()
        .copied()
        .filter(|&g| g < CHATTER_CEILING_MS)
        .max()?;
    let count_low = gaps.iter().filter(|&&g| g < CHATTER_CEILING_MS).count();
    if count_low < MIN_CHATTER_COUNT {
        return None;
    }
    Some(((max_low / BUCKET_MS) + 1) * BUCKET_MS)
}

pub fn format_report(snapshot: &HashMap<u32, Vec<u32>>) -> String {
    if snapshot.is_empty() {
        return String::from("[calibrate] no data yet\n");
    }
    let mut keys: Vec<u32> = snapshot.keys().copied().collect();
    keys.sort();
    let (chatter, clean): (Vec<u32>, Vec<u32>) = keys
        .into_iter()
        .filter(|vk| snapshot[vk].len() >= 3)
        .partition(|vk| suggest_from_gaps(&snapshot[vk]).is_some());

    let mut out = String::new();
    writeln!(
        out,
        "[calibrate] {} keys sampled — {} chatter candidate{}, {} clean",
        chatter.len() + clean.len(),
        chatter.len(),
        if chatter.len() == 1 { "" } else { "s" },
        clean.len(),
    )
    .unwrap();

    for vk in &chatter {
        let gaps = &snapshot[vk];
        let suggest = suggest_from_gaps(gaps).unwrap();
        let count_low = gaps.iter().filter(|&&g| g < CHATTER_CEILING_MS).count();
        writeln!(
            out,
            "\n  vk 0x{:02X} ({}): n={} — CHATTER: {} gap{} <{}ms, suggest threshold {}ms",
            vk,
            vk_name(*vk),
            gaps.len(),
            count_low,
            if count_low == 1 { "" } else { "s" },
            CHATTER_CEILING_MS,
            suggest,
        )
        .unwrap();
        write_histogram(&mut out, gaps);
    }

    if !clean.is_empty() {
        writeln!(out, "\n  clean (no chatter signature):").unwrap();
        for vk in &clean {
            let gaps = &snapshot[vk];
            let min = gaps.iter().copied().min().unwrap_or(0);
            writeln!(
                out,
                "    vk 0x{:02X} ({}): n={}, min gap {}ms",
                vk,
                vk_name(*vk),
                gaps.len(),
                min,
            )
            .unwrap();
        }
    }
    out
}

fn write_histogram(out: &mut String, gaps: &[u32]) {
    let mut buckets = [0u32; N_BUCKETS];
    let mut over = 0u32;
    for &g in gaps {
        let i = (g / BUCKET_MS) as usize;
        if i < N_BUCKETS {
            buckets[i] += 1;
        } else {
            over += 1;
        }
    }
    let max = *buckets.iter().max().unwrap_or(&1);
    for (i, &c) in buckets.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let bar_len = ((c as usize * 40) / max as usize).max(1);
        let bar = "#".repeat(bar_len);
        writeln!(
            out,
            "    {:>3}-{:>3}ms ({:>3}) {}",
            i as u32 * BUCKET_MS,
            (i as u32 + 1) * BUCKET_MS,
            c,
            bar
        )
        .unwrap();
    }
    if over > 0 {
        writeln!(out, "    >{MAX_MS}ms ({})", over).unwrap();
    }
}

fn vk_name(vk: u32) -> String {
    match vk {
        0x08 => "Back".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x10 => "Shift".into(),
        0x11 => "Ctrl".into(),
        0x12 => "Alt".into(),
        0x14 => "CapsLock".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x30..=0x39 | 0x41..=0x5A => format!("'{}'", vk as u8 as char),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("0x{vk:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_event_records_no_gap() {
        let mut c = Calibrator::new();
        c.record_down(0x42, 100);
        assert!(c.gaps.get(&0x42).is_none());
    }

    #[test]
    fn second_event_records_gap() {
        let mut c = Calibrator::new();
        c.record_down(0x42, 100);
        c.record_down(0x42, 140);
        assert_eq!(c.gaps[&0x42], vec![40]);
    }

    #[test]
    fn huge_gap_is_dropped() {
        // Gaps over 2 s aren't part of typing rhythm; treat as session break.
        let mut c = Calibrator::new();
        c.record_down(0x42, 100);
        c.record_down(0x42, 10_000);
        assert!(c.gaps.get(&0x42).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn suggest_finds_valley_between_clusters() {
        let mut c = Calibrator::new();
        // Chatter cluster at 10-25 ms
        for g in [10u32, 15, 20, 25, 15, 20, 12] {
            c.record_down(0x42, 0);
            c.record_down(0x42, g);
        }
        // Legit cluster at 80-120 ms
        for g in [80u32, 90, 100, 110, 120] {
            c.record_down(0x42, 10_000);
            c.record_down(0x42, 10_000 + g);
        }
        let t = c.suggest_threshold(0x42).unwrap();
        assert!((25..=80).contains(&t), "expected 25..=80, got {t}");
    }

    #[test]
    fn suggest_none_with_insufficient_data() {
        let mut c = Calibrator::new();
        c.record_down(0x42, 0);
        c.record_down(0x42, 50);
        assert!(c.suggest_threshold(0x42).is_none());
    }

    #[test]
    fn suggest_none_when_only_typing_range_gaps() {
        // A non-chattering key that got occasionally mashed fast — all gaps
        // above the 50 ms chatter ceiling shouldn't look like chatter, even
        // if the samples cluster in one typing bucket.
        let gaps: Vec<u32> = vec![180, 185, 190, 185, 190, 180];
        assert!(suggest_from_gaps(&gaps).is_none());
    }

    #[test]
    fn suggest_none_with_lone_chatter_sample() {
        // One stray sub-ceiling gap (e.g. a single rolled double-press) is
        // not enough to declare chatter.
        let gaps: Vec<u32> = vec![18, 180, 200, 210];
        assert!(suggest_from_gaps(&gaps).is_none());
    }
}
