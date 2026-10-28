// FastView - optional startup timing. Set FASTVIEW_TIMING=1 to collect a log in
// %TEMP%\fastview_timing.txt. Costs nothing when the variable is not set.

use std::io::Write;
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn start() {
    let _ = START.set(Instant::now());
}

pub fn mark(msg: &str) {
    if std::env::var_os("FASTVIEW_TIMING").is_none() {
        return;
    }
    let start = START.get().copied().unwrap_or_else(Instant::now);
    let line = format!("{:>6.3} ms  {}\n", start.elapsed().as_secs_f64() * 1000.0, msg);
    if let Some(dir) = std::env::var_os("TEMP") {
        let path = std::path::PathBuf::from(dir).join("fastview_timing.txt");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
