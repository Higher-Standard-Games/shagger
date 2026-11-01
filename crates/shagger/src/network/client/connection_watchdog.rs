//! SHAG `src/engine/network/client/ConnectionWatchdog.ts`: notices when a
//! connected client stops receiving snapshots.
//!
//! A WebSocket can stay open while nothing arrives: neither side sees an
//! error or a close, and the game simply stops. Snapshots arrive many times a
//! second, so a gap of a couple of seconds already means the link is not
//! delivering. A gap the client itself caused (a blocked loop) says nothing
//! about the connection, so the owner excuses it with [`skip_gap`](ConnectionWatchdog::skip_gap).

/// How far a stall has gone: `Warn` is a short stall, `Notice` one long
/// enough to treat the connection as lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallStage {
    Warn,
    Notice,
}

/// Times are milliseconds on a monotonic clock (`performance.now()`).
#[derive(Debug, Clone)]
pub struct ConnectionWatchdog {
    pub warn_after_ms: f64,
    pub notice_after_ms: f64,
    last_data_at: Option<f64>,
    reported: Option<StallStage>,
}

impl Default for ConnectionWatchdog {
    fn default() -> Self {
        Self::new(2000.0, 8000.0)
    }
}

impl ConnectionWatchdog {
    #[must_use]
    pub fn new(warn_after_ms: f64, notice_after_ms: f64) -> Self {
        Self {
            warn_after_ms,
            notice_after_ms,
            last_data_at: None,
            reported: None,
        }
    }

    /// Begin watching, counting from `now`.
    pub fn start(&mut self, now: f64) {
        self.last_data_at = Some(now);
        self.reported = None;
    }

    /// Stop watching, as on a disconnect.
    pub fn stop(&mut self) {
        self.last_data_at = None;
        self.reported = None;
    }

    /// Records that data arrived. Returns how long the stall it ends lasted,
    /// in ms, when that stall had been reported.
    pub fn data_arrived(&mut self, now: f64) -> Option<f64> {
        let last = self.last_data_at?;
        let stalled_for = self.reported.map(|_| now - last);
        self.last_data_at = Some(now);
        self.reported = None;
        stalled_for
    }

    /// The client was not running until now, so the gap so far is not a
    /// stall. A stall already reported stays reported.
    pub fn skip_gap(&mut self, now: f64) {
        if self.last_data_at.is_some() && self.reported.is_none() {
            self.last_data_at = Some(now);
        }
    }

    /// The stage a check at `now` newly reaches, at most once each per stall.
    pub fn check(&mut self, now: f64) -> Option<StallStage> {
        let silent_for = now - self.last_data_at?;
        if silent_for >= self.notice_after_ms && self.reported != Some(StallStage::Notice) {
            self.reported = Some(StallStage::Notice);
            return Some(StallStage::Notice);
        }
        if silent_for >= self.warn_after_ms && self.reported.is_none() {
            self.reported = Some(StallStage::Warn);
            return Some(StallStage::Warn);
        }
        None
    }

    /// Milliseconds since data last arrived; 0 when not watching.
    #[must_use]
    pub fn silent_for(&self, now: f64) -> f64 {
        self.last_data_at.map_or(0.0, |last| now - last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_each_stage_once_per_stall() {
        let mut w = ConnectionWatchdog::default();
        assert_eq!(w.check(100.0), None);
        w.start(0.0);
        assert_eq!(w.check(1999.0), None);
        assert_eq!(w.check(2000.0), Some(StallStage::Warn));
        assert_eq!(w.check(3000.0), None);
        assert_eq!(w.check(8000.0), Some(StallStage::Notice));
        assert_eq!(w.check(9000.0), None);
        assert_eq!(w.data_arrived(9500.0), Some(9500.0));
        assert_eq!(w.data_arrived(9600.0), None);
        // Straight to notice without a warn first.
        assert_eq!(w.check(20_000.0), Some(StallStage::Notice));
    }

    #[test]
    fn skipped_gaps_do_not_count_unless_already_reported() {
        let mut w = ConnectionWatchdog::default();
        w.start(0.0);
        w.skip_gap(5000.0);
        assert_eq!(w.check(6000.0), None);
        assert_eq!(w.check(7000.0), Some(StallStage::Warn));
        w.skip_gap(7500.0);
        assert_eq!(w.silent_for(7500.0), 7500.0 - 5000.0);
        w.stop();
        assert_eq!(w.silent_for(9999.0), 0.0);
    }
}
