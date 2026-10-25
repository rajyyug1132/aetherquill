//! Suspend/resume detection (TODOS P2). On Linux CLOCK_MONOTONIC stops while
//! the device is suspended and CLOCK_BOOTTIME keeps counting, so after a power
//! button nap the two drift apart by the time slept. The event loop samples
//! both every iteration; when the drift jumps, the panel's prior contents
//! can't be trusted and main.rs redraws everything from the stroke store.
//! Pure state, no clock reads here — host-testable.

/// Drift (seconds) that counts as a suspend. The loop polls every 20ms, so
/// normal scheduling jitter never gets near this.
pub const SUSPEND_DRIFT_S: f64 = 2.0;

#[derive(Debug, Default)]
pub struct ResumeDetector {
    /// BOOTTIME - MONOTONIC at the last sample.
    last_drift: Option<f64>,
}

impl ResumeDetector {
    /// Feed one (boottime, monotonic) sample in seconds. True exactly once per
    /// resume: the first sample after the drift grew by `SUSPEND_DRIFT_S`+.
    pub fn observe(&mut self, boottime_s: f64, monotonic_s: f64) -> bool {
        let drift = boottime_s - monotonic_s;
        let resumed = self.last_drift.is_some_and(|last| drift - last >= SUSPEND_DRIFT_S);
        self.last_drift = Some(drift);
        resumed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_never_reports_resume() {
        let mut d = ResumeDetector::default();
        // Device that already slept before launch: big drift, but no baseline yet.
        assert!(!d.observe(5000.0, 1200.0));
    }

    #[test]
    fn steady_running_never_reports_resume() {
        let mut d = ResumeDetector::default();
        let (mut boot, mut mono) = (100.0, 40.0);
        for _ in 0..1000 {
            boot += 0.02;
            mono += 0.02;
            assert!(!d.observe(boot, mono));
        }
    }

    #[test]
    fn long_poll_without_suspend_is_not_a_resume() {
        // Both clocks advance together (e.g. app stalled 10s): not a suspend.
        let mut d = ResumeDetector::default();
        d.observe(100.0, 40.0);
        assert!(!d.observe(110.0, 50.0));
    }

    #[test]
    fn suspend_reports_once_then_settles() {
        let mut d = ResumeDetector::default();
        d.observe(100.0, 40.0);
        // Slept 45s: boottime jumped, monotonic barely moved.
        assert!(d.observe(145.02, 40.02));
        assert!(!d.observe(145.04, 40.04), "same resume must not redraw twice");
        // A second nap is a second resume.
        assert!(d.observe(300.0, 40.06));
    }

    #[test]
    fn threshold_boundary() {
        let mut d = ResumeDetector::default();
        d.observe(0.0, 0.0);
        assert!(!d.observe(1.9, 0.0));
        let mut d = ResumeDetector::default();
        d.observe(0.0, 0.0);
        assert!(d.observe(SUSPEND_DRIFT_S, 0.0));
    }
}
