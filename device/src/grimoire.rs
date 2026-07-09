//! On-device spell history — a JSONL append log, one line per activation.
//! Riddle-inspired ("the diary remembers"), but far simpler: no read-modify-
//! write, just append. Lives entirely under /home/root/wha/, same safety
//! property as everything else (rm -rf /home/root/wha removes it too).

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_PATH: &str = "/home/root/wha/spells/log.jsonl";

/// Best-effort: a failed write (disk full, permission denied) must never
/// crash spell activation — logging is a nice-to-have, not the feature.
pub fn log_spell(element: &str, quality: f64, stability: f64, signature: &str) {
    let Some(dir) = std::path::Path::new(LOG_PATH).parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // ponytail: Rust's `{:?}` string escaping is JSON-compatible for the ASCII
    // element names and hex-ish signatures this ever sees — no serde_json
    // dependency needed for one log line. Revisit if element/signature ever
    // carry non-ASCII text.
    let line = format!(
        "{{\"t\":{timestamp},\"element\":{element:?},\"quality\":{:.3},\"stability\":{:.3},\"signature\":{signature:?}}}\n",
        quality, stability
    );

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG_PATH) {
        let _ = file.write_all(line.as_bytes());
    }
}
