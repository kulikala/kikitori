//! 環境変数 KIKITORI_PROFILE=1 のときだけ動く、工程ごとの所要時間の集計。
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type Totals = BTreeMap<&'static str, (Duration, u64)>;

fn totals() -> &'static Mutex<Totals> {
    static TOTALS: OnceLock<Mutex<Totals>> = OnceLock::new();
    TOTALS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("KIKITORI_PROFILE").ok().as_deref() == Some("1"))
}

pub(crate) fn dump_on_each_clip() -> bool {
    enabled()
}

/// 計測の開始。無効なら None を返し、以降の費用はほぼゼロ。
pub(crate) fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// `start` からの経過を工程名ごとに足す。
pub(crate) fn add(name: &'static str, at: Option<Instant>) {
    if let Some(at) = at {
        add_duration(name, at.elapsed());
    }
}

fn add_duration(name: &'static str, d: Duration) {
    let mut guard = totals().lock().unwrap();
    let e = guard.entry(name).or_insert((Duration::ZERO, 0));
    e.0 += d;
    e.1 += 1;
}

/// 集計を stderr に出して 0 に戻す。
pub(crate) fn dump_and_reset(label: &str) {
    if !enabled() {
        return;
    }
    let mut guard = totals().lock().unwrap();
    let mut rows: Vec<_> = guard.iter().map(|(k, v)| (*k, *v)).collect();
    guard.clear();
    drop(guard);
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    eprintln!("[kikitori-whisper profile] {label}");
    for (name, (d, n)) in rows {
        eprintln!(
            "  {name:<22} {:>9.3}s  ({n} calls, {:.3} ms/call)",
            d.as_secs_f64(),
            d.as_secs_f64() * 1000.0 / n.max(1) as f64
        );
    }
}
