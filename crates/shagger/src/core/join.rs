//! Waiting for a worker thread to end, but not forever. Not a port: the
//! browser had no threads to join.
//!
//! A network worker can be stuck in a name lookup the operating system will
//! not cut short (an offline machine, a captive portal); joining it from a
//! `Drop` on the game thread would freeze the window for as long. Past the
//! limit the thread is left to end on its own.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Joins `thread` if it ends within `limit`; otherwise detaches it.
/// `true` when it ended.
pub fn join_within(thread: JoinHandle<()>, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = thread.join();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stuck_thread_is_left_behind_and_a_quick_one_joined() {
        let quick = std::thread::spawn(|| {});
        assert!(join_within(quick, Duration::from_secs(5)));
        let stuck = std::thread::spawn(|| std::thread::sleep(Duration::from_secs(2)));
        let started = Instant::now();
        assert!(!join_within(stuck, Duration::from_millis(50)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
