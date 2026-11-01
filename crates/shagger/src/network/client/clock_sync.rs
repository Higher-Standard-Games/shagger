//! SHAG `src/engine/network/client/ClockSync.ts`: NTP-like server time
//! synchronization.
//!
//! The client sends `Ping { clientSendTime = Date.now() }`; the server
//! answers `Pong { clientSendTime, serverTime }`. Then
//! `offset = serverTime - (clientSendTime + clientRecvTime) / 2`. The best
//! 75% of up to 16 samples by round trip are averaged.
//!
//! The TypeScript drives its pings with `setTimeout` and `setInterval`; here
//! the owner calls [`ClockSync::poll`] with the time and sends what it
//! returns, so the logic needs no runtime. Wall-clock times are `Date.now()`
//! milliseconds; [`ClockSync::to_local_time`] answers on the monotonic clock
//! the owner passes as `date_to_perf`.

use shagger_shared::net::message_codec::encode_ping;

/// Pings per sync burst.
pub const SYNC_BURST_COUNT: u32 = 5;
/// Spacing of the pings in a burst (ms).
pub const SYNC_BURST_INTERVAL: f64 = 100.0;
/// One ping this often to track drift (ms).
pub const RESYNC_INTERVAL: f64 = 30_000.0;
/// Samples kept in the rolling window.
pub const MAX_SAMPLES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSyncSample {
    pub offset: f64,
    pub rtt: f64,
}

#[derive(Debug, Clone)]
pub struct ClockSync {
    offset: f64,
    rtt: f64,
    synced: bool,
    samples: Vec<ClockSyncSample>,
    running: bool,
    burst_remaining: u32,
    next_burst_ping: f64,
    next_resync: f64,
    /// `Date.now() - performance.now()` at construction.
    date_to_perf: f64,
}

impl ClockSync {
    /// `date_to_perf` is `Date.now() - performance.now()` for the owner's clocks.
    #[must_use]
    pub fn new(date_to_perf: f64) -> Self {
        Self {
            offset: 0.0,
            rtt: 0.0,
            synced: false,
            samples: Vec::new(),
            running: false,
            burst_remaining: 0,
            next_burst_ping: 0.0,
            next_resync: 0.0,
            date_to_perf,
        }
    }

    /// `startSync`: a burst now, then one ping every [`RESYNC_INTERVAL`].
    pub fn start_sync(&mut self, now: f64) {
        self.running = true;
        self.burst_remaining = SYNC_BURST_COUNT;
        self.next_burst_ping = now;
        self.next_resync = now + RESYNC_INTERVAL;
    }

    /// `stop`: no more pings until the next [`start_sync`](Self::start_sync).
    pub fn stop(&mut self) {
        self.running = false;
        self.burst_remaining = 0;
    }

    /// The pings due at wall-clock time `now`, encoded. They go unreliable.
    pub fn poll(&mut self, now: f64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if !self.running {
            return out;
        }
        while self.burst_remaining > 0 && now >= self.next_burst_ping {
            out.push(encode_ping(now));
            self.burst_remaining -= 1;
            self.next_burst_ping += SYNC_BURST_INTERVAL;
        }
        while now >= self.next_resync {
            out.push(encode_ping(now));
            self.next_resync += RESYNC_INTERVAL;
        }
        out
    }

    /// `processPong`, received at wall-clock time `client_recv_time`.
    pub fn process_pong(&mut self, client_send_time: f64, server_time: f64, client_recv_time: f64) {
        let rtt = client_recv_time - client_send_time;
        // Reject stale or impossible pongs. Written as the TypeScript compares,
        // which lets NaN through; a range check would not.
        #[allow(clippy::manual_range_contains)]
        if rtt < 0.0 || rtt > 5000.0 {
            return;
        }
        let offset = server_time - (client_send_time + client_recv_time) / 2.0;
        self.samples.push(ClockSyncSample { offset, rtt });
        if self.samples.len() > MAX_SAMPLES {
            self.samples.remove(0);
        }
        self.rtt = rtt;
        self.recompute_offset();
        self.synced = true;
    }

    /// Local `Date.now()` to estimated server time.
    #[must_use]
    pub fn to_server_time(&self, local_time: f64) -> f64 {
        local_time + self.offset
    }

    /// Server `Date.now()` to the local monotonic clock, for interpolation.
    #[must_use]
    pub fn to_local_time(&self, server_time: f64) -> f64 {
        server_time - self.offset - self.date_to_perf
    }

    /// Estimated offset in ms (server − local).
    #[must_use]
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// Best round trip in the window, in ms.
    #[must_use]
    pub fn rtt(&self) -> f64 {
        self.rtt
    }

    #[must_use]
    pub fn is_synced(&self) -> bool {
        self.synced
    }

    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    fn recompute_offset(&mut self) {
        if self.samples.is_empty() {
            return;
        }
        // Array.prototype.sort is stable, as is sort_by.
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.rtt.total_cmp(&b.rtt));
        let keep = ((sorted.len() as f64 * 0.75).ceil() as usize).max(1);
        let best = &sorted[..keep];
        self.offset = best.iter().fold(0.0, |sum, s| sum + s.offset) / best.len() as f64;
        self.rtt = sorted[0].rtt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_then_resyncs() {
        let mut c = ClockSync::new(0.0);
        assert!(c.poll(0.0).is_empty());
        c.start_sync(1000.0);
        assert_eq!(c.poll(1000.0).len(), 1);
        assert_eq!(c.poll(1050.0).len(), 0);
        assert_eq!(c.poll(1100.0).len(), 1);
        assert_eq!(c.poll(1450.0).len(), 3);
        assert_eq!(c.poll(30_999.0).len(), 0);
        assert_eq!(c.poll(31_000.0).len(), 1);
        c.stop();
        assert!(c.poll(100_000.0).is_empty());
    }

    #[test]
    fn averages_the_best_three_quarters() {
        let mut c = ClockSync::new(0.0);
        // rtt 10, offset 100; rtt 20, offset 200; rtt 30, offset 300; rtt 400, offset 9000
        c.process_pong(0.0, 105.0, 10.0);
        c.process_pong(0.0, 210.0, 20.0);
        c.process_pong(0.0, 315.0, 30.0);
        c.process_pong(0.0, 9200.0, 400.0);
        assert!(c.is_synced());
        assert_eq!(c.offset(), 200.0);
        assert_eq!(c.rtt(), 10.0);
        c.process_pong(0.0, 0.0, 6000.0);
        c.process_pong(10.0, 0.0, 5.0);
        assert_eq!(c.sample_count(), 4);
        assert_eq!(c.to_server_time(1000.0), 1200.0);
        assert_eq!(c.to_local_time(1200.0), 1000.0);
    }
}
