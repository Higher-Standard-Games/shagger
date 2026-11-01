//! The I/O around [`NetworkGameHost`]: a WebSocket to the game server on a
//! network thread of its own, talking to the game through channels (RUST.md
//! 4.1). It has no TypeScript counterpart; it does what the browser's event
//! loop does for `NetworkGameHost.ts` and `WebSocketTransport.ts`: open the
//! socket (`ws://`, or `wss://` over rustls with the bundled Mozilla roots),
//! pass binary frames in, send what the host returns, run the host's timers,
//! and on a drop take the seat back with the resume token
//! (`resumeSession`), retrying every [`RESUME_RETRY_MS`] until the server's
//! grace has passed.
//!
//! The game calls [`NetworkSession::poll_events`] once a frame and
//! [`NetworkSession::send`] for its requests. Neither blocks.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use shagger_shared::net::command::Command;
use shagger_shared::net::message_codec::ClientPrefs;
use shagger_shared::net::snapshot::EventCodec;
use tokio::sync::mpsc as async_mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::connection_watchdog::StallStage;
use super::hosts::network_game_host::{HostEvent, HostPhase, NetworkGameHost, Outgoing, RESUME_RETRY_MS};

/// How often the host's timers run (clock-sync bursts are 100 ms apart).
const POLL_MS: u64 = 50;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the game asks of the connection.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionRequest {
    Command(Command),
    JoinGame {
        team_id: Option<String>,
        manual_pick: bool,
        admission_ticket: Option<String>,
    },
    Spectate,
    Chat {
        text: String,
        team_only: bool,
    },
    ClientPrefs(ClientPrefs),
    Loadout {
        primary: String,
        secondary: String,
    },
    /// Close the connection and give up the seat.
    Disconnect,
}

/// What the connection tells the game.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent<E> {
    Host(HostEvent<E>),
    /// The connection dropped and the session is taking the seat back.
    Resuming {
        attempt: u32,
    },
    /// The session is over; nothing more will arrive.
    Closed(String),
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// `ws://` or `wss://` (`http(s)://` is rewritten as the browser does).
    pub url: String,
    pub player_name: String,
    /// The lobby's single-use seat credential, when joining through the lobby.
    pub admission_ticket: Option<String>,
}

pub struct NetworkSession<E> {
    requests: async_mpsc::UnboundedSender<SessionRequest>,
    events: mpsc::Receiver<SessionEvent<E>>,
    thread: Option<thread::JoinHandle<()>>,
}

fn wall_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

impl<E: Send + 'static> NetworkSession<E> {
    /// Opens the connection on a new thread and starts the handshake.
    ///
    /// # Panics
    /// When the thread or its runtime cannot be created.
    pub fn start<C>(config: SessionConfig, codec: C) -> Self
    where
        C: EventCodec<Event = E> + Send + 'static,
    {
        let (request_tx, request_rx) = async_mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("shagger-net".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("network runtime");
                let reason = runtime.block_on(run(config, codec, request_rx, &event_tx));
                let _ = event_tx.send(SessionEvent::Closed(reason));
            })
            .expect("network thread");
        Self {
            requests: request_tx,
            events: event_rx,
            thread: Some(thread),
        }
    }

    /// Everything that arrived since the last call.
    pub fn poll_events(&self) -> Vec<SessionEvent<E>> {
        self.events.try_iter().collect()
    }

    /// Waits up to `timeout` for the next event.
    pub fn next_event(&self, timeout: Duration) -> Option<SessionEvent<E>> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Queues a request; ignored once the session has closed.
    pub fn send(&self, request: SessionRequest) {
        let _ = self.requests.send(request);
    }
}

impl<E> Drop for NetworkSession<E> {
    fn drop(&mut self) {
        let _ = self.requests.send(SessionRequest::Disconnect);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn open(url: &str) -> Result<Socket, String> {
    // WebSocketTransport: url.replace(/^http/, "ws")
    let url = url
        .strip_prefix("http")
        .map_or_else(|| url.to_owned(), |rest| format!("ws{rest}"));
    // Nagle off: acks and commands are small and must leave at once (ping is read from acks).
    let connect = tokio_tungstenite::connect_async_with_config(url.as_str(), None, true);
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(Ok((socket, _response))) => Ok(socket),
        Ok(Err(e)) => Err(format!("WebSocket connection error: {e}")),
        Err(_) => Err("WebSocket connection timed out".into()),
    }
}

/// How one socket's life ended.
enum Ended {
    /// The game asked to disconnect, or the host failed: the session is over.
    Done(String),
    /// The socket closed or stalled; resuming may take the seat back.
    Dropped(String),
}

async fn run<C: EventCodec>(
    config: SessionConfig,
    codec: C,
    mut requests: async_mpsc::UnboundedReceiver<SessionRequest>,
    events: &mpsc::Sender<SessionEvent<C::Event>>,
) -> String {
    let start = Instant::now();
    let mono = move || start.elapsed().as_secs_f64() * 1000.0;
    let mut host = NetworkGameHost::new(codec, wall_ms() - mono());

    let mut socket = match open(&config.url).await {
        Ok(s) => s,
        Err(why) => {
            let _ = events.send(SessionEvent::Host(HostEvent::Failed(why.clone())));
            return why;
        }
    };
    let join = host.connect(&config.player_name, config.admission_ticket.as_deref(), mono());
    if let Err(e) = socket.send(Message::binary(join.data)).await {
        return format!("send failed: {e}");
    }

    loop {
        let ended = pump(&mut socket, &mut host, &mut requests, events, &mono).await;
        let _ = socket.close(None).await;
        let why = match ended {
            Ended::Done(why) => return why,
            Ended::Dropped(why) => why,
        };
        if !host.transport_closed() {
            return why;
        }
        // resumeSession: reconnect and claim the seat until the grace is spent.
        let (_, grace_ms) = host.resume_token().expect("transport_closed checked the token");
        let deadline = mono() + grace_ms;
        let mut attempt = 0;
        socket = loop {
            if mono() >= deadline {
                let why = "The match connection could not be restored. Please return to the lobby and join again.";
                let _ = events.send(SessionEvent::Host(HostEvent::Failed(why.into())));
                return why.into();
            }
            attempt += 1;
            let _ = events.send(SessionEvent::Resuming { attempt });
            if let Ok(mut s) = open(&config.url).await {
                if let Some(resume) = host.resume(mono()) {
                    if s.send(Message::binary(resume.data)).await.is_ok() {
                        break s;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(RESUME_RETRY_MS as u64)).await;
        };
    }
}

async fn send_all(socket: &mut Socket, out: &mut Vec<Outgoing>) -> Result<(), String> {
    for message in out.drain(..) {
        socket
            .feed(Message::binary(message.data))
            .await
            .map_err(|e| format!("send failed: {e}"))?;
    }
    socket.flush().await.map_err(|e| format!("send failed: {e}"))
}

async fn pump<C: EventCodec>(
    socket: &mut Socket,
    host: &mut NetworkGameHost<C>,
    requests: &mut async_mpsc::UnboundedReceiver<SessionRequest>,
    events: &mpsc::Sender<SessionEvent<C::Event>>,
    mono: &impl Fn() -> f64,
) -> Ended {
    let mut timer = tokio::time::interval(Duration::from_millis(POLL_MS));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut out = Vec::new();
    let mut found = Vec::new();
    loop {
        tokio::select! {
            incoming = socket.next() => match incoming {
                Some(Ok(Message::Binary(data))) => host.handle_message(&data, wall_ms(), mono(), &mut out, &mut found),
                Some(Ok(Message::Close(frame))) => {
                    return Ended::Dropped(frame.map_or_else(|| "closed".into(), |f| f.reason.to_string()));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Ended::Dropped(e.to_string()),
                None => return Ended::Dropped("closed".into()),
            },
            request = requests.recv() => {
                let message = match request {
                    None | Some(SessionRequest::Disconnect) => {
                        host.disconnect();
                        return Ended::Done("disconnected".into());
                    }
                    Some(SessionRequest::Command(c)) => host.send_command(&c),
                    Some(SessionRequest::JoinGame { team_id, manual_pick, admission_ticket }) => {
                        host.send_join_game(team_id.as_deref(), manual_pick, admission_ticket.as_deref())
                    }
                    Some(SessionRequest::Spectate) => host.send_spectate(),
                    Some(SessionRequest::Chat { text, team_only }) => host.send_chat(&text, team_only),
                    Some(SessionRequest::ClientPrefs(p)) => host.set_client_prefs(p),
                    Some(SessionRequest::Loadout { primary, secondary }) => host.send_loadout(&primary, &secondary),
                };
                out.extend(message);
            },
            _ = timer.tick() => host.poll(wall_ms(), mono(), &mut out, &mut found),
        }
        if let Err(why) = send_all(socket, &mut out).await {
            return Ended::Dropped(why);
        }
        let mut stalled = false;
        for event in found.drain(..) {
            stalled |= matches!(
                event,
                HostEvent::Stall {
                    stage: StallStage::Notice,
                    ..
                }
            );
            let _ = events.send(SessionEvent::Host(event));
        }
        if host.phase() == HostPhase::Failed {
            return Ended::Done("connection failed".into());
        }
        // A stalled link carries nothing either way and neither side closes
        // it: drop it and claim the seat the server still holds.
        if stalled && host.resume_token().is_some() {
            return Ended::Dropped("stalled".into());
        }
    }
}
