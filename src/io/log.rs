//! 可选诊断日志（设置里总开关）。默认关闭，避免拖慢输入。
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static ENABLED: AtomicBool = AtomicBool::new(false);
static ORIGIN: Mutex<Option<Instant>> = Mutex::new(None);

const SLOW_UI_MS: f64 = 50.0;
const SLOW_SPAN_MS: f64 = 20.0;
const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        let _ = ORIGIN.lock().map(|mut g| {
            if g.is_none() {
                *g = Some(Instant::now());
            }
        });
        write("logging on");
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn file_path() -> PathBuf {
    super::settings::data_dir().join("ui.log")
}

/// 相对启动时间的秒数，便于对照卡顿。
fn rel_secs() -> f64 {
    ORIGIN
        .lock()
        .ok()
        .and_then(|g| *g)
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0)
}

fn wall_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn write(msg: &str) {
    if !enabled() {
        return;
    }
    let path = file_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let bak = path.with_extension("log.old");
            let _ = std::fs::remove_file(&bak);
            let _ = std::fs::rename(&path, &bak);
        }
    }
    let line = format!("t={:.3}s utc_ms={} {}\n", rel_secs(), wall_ms(), msg);
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = f.write_all(line.as_bytes());
    let _ = f.flush();
}

pub fn slow(name: &str, t0: Instant, threshold_ms: f64) {
    if !enabled() {
        return;
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if ms >= threshold_ms {
        write(&format!("{name} {ms:.0}ms"));
    }
}

pub fn ui_lag(t0: Instant, extra: &str) {
    if !enabled() {
        return;
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if ms >= SLOW_UI_MS {
        if extra.is_empty() {
            write(&format!("ui lag {ms:.0}ms"));
        } else {
            write(&format!("ui lag {ms:.0}ms {extra}"));
        }
    }
}

pub const SPAN_MS: f64 = SLOW_SPAN_MS;
