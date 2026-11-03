//! Fetching a file over HTTP(S) with the deadlines the browser client puts
//! on `fetch` (wash-fps `client/DemoDownload.ts`'s idle and total timers),
//! on a thread of its own so the frame loop only polls. The browser's
//! `fetch` has no TypeScript counterpart to port; the deadlines and the
//! errors follow the game's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;
use std::time::Duration;

use super::tls;

/// Why a fetch ended without the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The server answered with this status.
    Status(u16),
    /// Nothing arrived for the idle deadline.
    Stalled,
    /// The whole transfer passed the total deadline.
    TooLong,
    Cancelled,
    /// Could not connect, or the transfer broke.
    Network(String),
}

/// What a fetch reports while it runs.
#[derive(Debug)]
pub enum FetchEvent {
    /// Bytes so far, and the length the server gave.
    Progress {
        received: u64,
        total: Option<u64>,
    },
    Done(Vec<u8>),
    Failed(FetchError),
}

/// One fetch in flight.
#[derive(Debug)]
pub struct HttpFetch {
    events: Receiver<FetchEvent>,
    cancel: Arc<AtomicBool>,
}

impl HttpFetch {
    /// Starts fetching `url`: `idle` is how long the transfer may go quiet,
    /// `total` how long it may take.
    #[must_use]
    pub fn start(url: &str, idle: Duration, total: Duration) -> Self {
        let (tx, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let url = url.to_owned();
        let stop = cancel.clone();
        thread::spawn(move || {
            let result = fetch(&url, idle, total, &stop, &mut |received, length| {
                let _ = tx.send(FetchEvent::Progress {
                    received,
                    total: length,
                });
            });
            let _ = tx.send(match result {
                Ok(bytes) => FetchEvent::Done(bytes),
                Err(e) => FetchEvent::Failed(e),
            });
        });
        Self { events, cancel }
    }

    /// The next event, if one is waiting.
    pub fn poll(&self) -> Option<FetchEvent> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(FetchEvent::Failed(FetchError::Cancelled)),
        }
    }

    /// Stops the transfer; it reports `Cancelled`.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for HttpFetch {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn classify(e: &reqwest::Error, total_elapsed: bool) -> FetchError {
    if e.is_timeout() {
        if total_elapsed {
            FetchError::TooLong
        } else {
            FetchError::Stalled
        }
    } else {
        FetchError::Network(e.to_string())
    }
}

/// The fetch itself, on the calling thread.
///
/// # Errors
/// Any [`FetchError`].
pub fn fetch(
    url: &str,
    idle: Duration,
    total: Duration,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, FetchError> {
    fetch_at_most(url, idle, total, None, cancel, progress)
}

/// What a body may reserve up front from the server's own length: a wrong
/// `Content-Length` must not make the allocation itself fail.
const MAX_RESERVE: u64 = 64 * 1024 * 1024;

/// [`fetch`], refusing a body longer than `max` bytes (a package whose size
/// the index gives) as soon as it passes it.
///
/// # Errors
/// Any [`FetchError`]; a body over `max` is [`FetchError::Network`].
pub fn fetch_at_most(
    url: &str,
    idle: Duration,
    total: Duration,
    max: Option<u64>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, FetchError> {
    let too_long = |n: usize| max.is_some_and(|m| n as u64 > m);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured((*tls::config()).clone())
            .read_timeout(idle)
            .connect_timeout(idle)
            .build()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let started = tokio::time::Instant::now();
        let deadline = started + total;
        let over = || tokio::time::Instant::now() >= deadline;
        let request = client.get(url).send();
        let mut response = tokio::time::timeout_at(deadline, request)
            .await
            .map_err(|_| FetchError::TooLong)?
            .map_err(|e| classify(&e, over()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let length = response.content_length();
        if length.is_some_and(|l| too_long(usize::try_from(l).unwrap_or(usize::MAX))) {
            return Err(FetchError::Network("the body is longer than expected".into()));
        }
        let reserve = length.unwrap_or(0).min(max.unwrap_or(MAX_RESERVE)).min(MAX_RESERVE);
        let mut data = Vec::with_capacity(usize::try_from(reserve).unwrap_or(0));
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(FetchError::Cancelled);
            }
            // Check cancelling at least once a second while a chunk is awaited.
            let wait = deadline.min(tokio::time::Instant::now() + Duration::from_secs(1));
            match tokio::time::timeout_at(wait, response.chunk()).await {
                Err(_) if over() => return Err(FetchError::TooLong),
                Err(_) => {
                    // No chunk this second: the read timeout decides stalls.
                    if cancel.load(Ordering::Relaxed) {
                        return Err(FetchError::Cancelled);
                    }
                    let chunk = tokio::time::timeout_at(deadline, response.chunk())
                        .await
                        .map_err(|_| FetchError::TooLong)?
                        .map_err(|e| classify(&e, over()))?;
                    let Some(chunk) = chunk else { break };
                    data.extend_from_slice(&chunk);
                    if too_long(data.len()) {
                        return Err(FetchError::Network("the body is longer than expected".into()));
                    }
                }
                Ok(chunk) => {
                    let Some(chunk) = chunk.map_err(|e| classify(&e, over()))? else {
                        break;
                    };
                    data.extend_from_slice(&chunk);
                    if too_long(data.len()) {
                        return Err(FetchError::Network("the body is longer than expected".into()));
                    }
                }
            }
            progress(data.len() as u64, length);
        }
        Ok(data)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A one-request HTTP server answering `body` with `status`.
    fn serve(status: &'static str, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        });
        format!("http://{addr}/file")
    }

    #[test]
    fn a_file_arrives_and_a_refusal_names_its_status() {
        let never = AtomicBool::new(false);
        let mut seen = 0;
        let bytes = fetch(
            &serve("200 OK", b"demo bytes"),
            Duration::from_secs(5),
            Duration::from_secs(10),
            &never,
            &mut |received, _| seen = received,
        )
        .unwrap();
        assert_eq!(bytes, b"demo bytes");
        assert_eq!(seen, 10);
        let refused = fetch(
            &serve("404 Not Found", b""),
            Duration::from_secs(5),
            Duration::from_secs(10),
            &never,
            &mut |_, _| {},
        );
        assert_eq!(refused, Err(FetchError::Status(404)));
    }

    #[test]
    fn a_background_fetch_reports_done() {
        let fetch = HttpFetch::start(
            &serve("200 OK", b"abc"),
            Duration::from_secs(5),
            Duration::from_secs(10),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match fetch.poll() {
                Some(FetchEvent::Done(bytes)) => {
                    assert_eq!(bytes, b"abc");
                    break;
                }
                Some(FetchEvent::Failed(e)) => panic!("{e:?}"),
                _ => assert!(std::time::Instant::now() < deadline),
            }
            thread::yield_now();
        }
    }
}
