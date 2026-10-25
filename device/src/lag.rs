//! Pen-to-ink lag log: per inked pen sample, kernel event time -> the moment
//! the covering update_partial was sent to qtfb. That send is the last point
//! the app can observe; AppLoad's server and the e-ink waveform come after it.
//! main.rs appends one JSON line per stroke to LOG_PATH, only if that file
//! already exists (`touch` it to enable, `rm` it to disable).

pub const LOG_PATH: &str = "/home/root/wha/lag.jsonl";

/// Nearest-rank percentile (p in 0..=100) of `ms`; sorts in place. 0 if empty.
pub fn percentile(ms: &mut [f64], p: f64) -> f64 {
    if ms.is_empty() {
        return 0.0;
    }
    ms.sort_by(|a, b| a.total_cmp(b));
    let rank = ((p / 100.0) * ms.len() as f64).ceil().max(1.0) as usize;
    ms[rank.min(ms.len()) - 1]
}

/// One stroke's log line. `busy` = samples flushed while recognition ran.
pub fn stroke_line(stroke: u64, lat_ms: &mut [f64], busy: usize) -> String {
    let (p50, p95) = (percentile(lat_ms, 50.0), percentile(lat_ms, 95.0));
    let max = lat_ms.last().copied().unwrap_or(0.0);
    let lat: Vec<String> = lat_ms.iter().map(|v| format!("{v:.2}")).collect();
    format!(
        "{{\"stroke\":{stroke},\"n\":{},\"busy\":{busy},\"p50\":{p50:.2},\"p95\":{p95:.2},\"max\":{max:.2},\"lat_ms\":[{}]}}",
        lat_ms.len(),
        lat.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let mut v: Vec<f64> = (1..=20).rev().map(f64::from).collect();
        assert_eq!(percentile(&mut v, 50.0), 10.0);
        assert_eq!(percentile(&mut v, 95.0), 19.0);
        assert_eq!(percentile(&mut v, 100.0), 20.0);
        assert_eq!(percentile(&mut [], 95.0), 0.0);
        let line = stroke_line(3, &mut [4.0, 2.0], 1);
        assert!(line.starts_with("{\"stroke\":3,\"n\":2,\"busy\":1,\"p50\":2.00,\"p95\":4.00,\"max\":4.00"));
        serde_json::from_str::<serde_json::Value>(&line).expect("valid JSON");
    }
}
