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
/// A send the link has not taken by then counts as a drop.
const SEND_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

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
    /// Back into play after a death (`RequestSpawn`).
    RequestSpawn,
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
    let connect =
        tokio_tungstenite::connect_async_tls_with_config(url.as_str(), None, true, Some(super::tls::connector()));
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

/// The resume in progress: when the server's grace runs out, and when the
/// current attempt gives up waiting for `JoinAccept`.
#[derive(Clone, Copy)]
struct Resume {
    deadline: f64,
    attempt: u32,
    answer_by: f64,
}

/// Waits for the game to ask for the end, dropping its other requests: no
/// link carries them while the session is reopening.
async fn until_disconnect(requests: &mut async_mpsc::UnboundedReceiver<SessionRequest>) {
    while let Some(request) = requests.recv().await {
        if request == SessionRequest::Disconnect {
            return;
        }
    }
}

/// Closes the socket without waiting on a link that carries nothing.
async fn close(socket: &mut Socket) {
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, socket.close(None)).await;
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

    let opened = tokio::select! {
        opened = open(&config.url) => opened,
        () = until_disconnect(&mut requests) => return "disconnected".into(),
    };
    let mut socket = match opened {
        Ok(s) => s,
        Err(why) => {
            let _ = events.send(SessionEvent::Host(HostEvent::Failed(why.clone())));
            return why;
        }
    };
    let join = host.connect(&config.player_name, config.admission_ticket.as_deref(), mono());
    if let Err(why) = send_all(&mut socket, &mut vec![join]).await {
        return why;
    }

    let mut resume: Option<Resume> = None;
    loop {
        let ended = pump(&mut socket, &mut host, &mut requests, events, &mono, &mut resume).await;
        close(&mut socket).await;
        let why = match ended {
            Ended::Done(why) => return why,
            Ended::Dropped(why) => why,
        };
        // A drop while resuming only fails that attempt (resumeSession keeps
        // trying until the grace is spent); a drop before the join completed
        // ends the session.
        let resumable = host.transport_closed();
        if resume.is_none() {
            if !resumable {
                return why;
            }
            let (_, grace_ms) = host.resume_token().expect("transport_closed checked the token");
            resume = Some(Resume {
                deadline: mono() + grace_ms,
                attempt: 0,
                answer_by: 0.0,
            });
        }
        let Some(r) = resume.as_mut() else { unreachable!() };
        // resumeSession: reconnect and claim the seat until the grace is spent.
        socket = loop {
            if mono() >= r.deadline || host.resume_token().is_none() {
                let why = "The match connection could not be restored. Please return to the lobby and join again.";
                let _ = events.send(SessionEvent::Host(HostEvent::Failed(why.into())));
                return why.into();
            }
            r.attempt += 1;
            let _ = events.send(SessionEvent::Resuming { attempt: r.attempt });
            let opened = tokio::select! {
                opened = open(&config.url) => opened,
                () = until_disconnect(&mut requests) => return "disconnected".into(),
            };
            if let Ok(mut s) = opened
                && let Some(message) = host.resume(mono())
            {
                if send_all(&mut s, &mut vec![message]).await.is_ok() {
                    r.answer_by = mono() + RESUME_RETRY_MS;
                    break s;
                }
                close(&mut s).await;
            }
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(RESUME_RETRY_MS as u64)) => {}
                () = until_disconnect(&mut requests) => return "disconnected".into(),
            }
        };
    }
}

/// Sends what the host returned, giving up on a link that takes none of it.
async fn send_all(socket: &mut Socket, out: &mut Vec<Outgoing>) -> Result<(), String> {
    let send = async {
        for message in out.drain(..) {
            socket
                .feed(Message::binary(message.data))
                .await
                .map_err(|e| format!("send failed: {e}"))?;
        }
        socket.flush().await.map_err(|e| format!("send failed: {e}"))
    };
    tokio::time::timeout(SEND_TIMEOUT, send)
        .await
        .unwrap_or_else(|_| Err("send stalled".into()))
}

async fn pump<C: EventCodec>(
    socket: &mut Socket,
    host: &mut NetworkGameHost<C>,
    requests: &mut async_mpsc::UnboundedReceiver<SessionRequest>,
    events: &mpsc::Sender<SessionEvent<C::Event>>,
    mono: &impl Fn() -> f64,
    resume: &mut Option<Resume>,
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
                    Some(SessionRequest::RequestSpawn) => host.send_request_spawn(),
                    Some(SessionRequest::Chat { text, team_only }) => host.send_chat(&text, team_only),
                    Some(SessionRequest::ClientPrefs(p)) => host.set_client_prefs(p),
                    Some(SessionRequest::Loadout { primary, secondary }) => host.send_loadout(&primary, &secondary),
                };
                out.extend(message);
            },
            _ = timer.tick() => host.poll(wall_ms(), mono(), &mut out, &mut found),
        }
        if let Some(r) = resume {
            if host.is_connected() {
                *resume = None;
            } else if mono() >= r.answer_by {
                // resumeSession waits RESUME_RETRY_MS for JoinAccept, then tries again.
                return Ended::Dropped("no answer to the resume".into());
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use shagger_shared::net::message_codec::{
        JoinAccept, MessageType, ResumeToken, Welcome, encode_join_accept, encode_resume_token, encode_welcome,
        peek_message_type,
    };
    use shagger_shared::net::snapshot::NoEvents;
    use tokio::net::TcpListener;

    type Server = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

    async fn accept(listener: &TcpListener) -> Server {
        let (tcp, _) = listener.accept().await.unwrap();
        tokio_tungstenite::accept_async(tcp).await.unwrap()
    }

    /// The first message's type.
    async fn first(ws: &mut Server) -> u8 {
        loop {
            if let Some(Ok(Message::Binary(data))) = ws.next().await {
                return peek_message_type(&data).unwrap();
            }
        }
    }

    async fn admit(ws: &mut Server) {
        let welcome = encode_welcome(&Welcome {
            server_id: "s".into(),
            server_name: "Test".into(),
            map: "washoi_dm2".into(),
            tick_rate: 125,
            max_players: 16,
            current_players: 1,
        });
        let accept = encode_join_accept(&JoinAccept {
            client_id: "c1".into(),
            player_id: "player_1".into(),
            entity_id: "player_1".into(),
        });
        let token = encode_resume_token(&ResumeToken {
            token: "tok".into(),
            grace_ms: 8000,
        });
        for message in [welcome, accept, token] {
            ws.send(Message::binary(message)).await.unwrap();
        }
    }

    /// A server on its own thread, running `script` on its listener.
    fn serve<F, Fut>(script: F) -> String
    where
        F: FnOnce(TcpListener) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()>,
    {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(format!("ws://{}", listener.local_addr().unwrap())).unwrap();
                script(listener).await;
            });
        });
        rx.recv().unwrap()
    }

    fn start(url: String) -> NetworkSession<std::convert::Infallible> {
        NetworkSession::start(
            SessionConfig {
                url,
                player_name: "Rusty".into(),
                admission_ticket: None,
            },
            NoEvents,
        )
    }

    /// The events until `stop` matches one, or five seconds pass.
    fn events_until(
        session: &NetworkSession<std::convert::Infallible>,
        stop: impl Fn(&SessionEvent<std::convert::Infallible>) -> bool,
    ) -> Vec<SessionEvent<std::convert::Infallible>> {
        let mut seen = Vec::new();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if let Some(event) = session.next_event(Duration::from_millis(50)) {
                let done = stop(&event);
                seen.push(event);
                if done {
                    break;
                }
            }
        }
        seen
    }

    fn joined(event: &SessionEvent<std::convert::Infallible>) -> bool {
        matches!(event, SessionEvent::Host(HostEvent::Joined { .. }))
    }

    #[test]
    fn a_resume_dropped_before_the_accept_is_tried_again() {
        let url = serve(|listener| async move {
            let mut ws = accept(&listener).await;
            assert_eq!(first(&mut ws).await, MessageType::Join as u8);
            admit(&mut ws).await;
            drop(ws);
            // The first resume reaches a server that goes away before answering.
            let mut ws = accept(&listener).await;
            assert_eq!(first(&mut ws).await, MessageType::Resume as u8);
            drop(ws);
            let mut ws = accept(&listener).await;
            assert_eq!(first(&mut ws).await, MessageType::Resume as u8);
            admit(&mut ws).await;
            while ws.next().await.is_some() {}
        });
        let session = start(url);
        assert!(events_until(&session, joined).iter().any(joined));
        let seen = events_until(&session, joined);
        let attempts: Vec<u32> = seen
            .iter()
            .filter_map(|e| match e {
                SessionEvent::Resuming { attempt } => Some(*attempt),
                _ => None,
            })
            .collect();
        assert_eq!(attempts, [1, 2]);
        assert!(joined(seen.last().unwrap()), "{seen:?}");
    }

    #[test]
    fn quitting_while_resuming_does_not_wait_for_the_grace() {
        let url = serve(|listener| async move {
            let mut ws = accept(&listener).await;
            first(&mut ws).await;
            admit(&mut ws).await;
            drop(ws);
            // Nobody listens any more: every resume attempt is refused.
            drop(listener);
            std::future::pending::<()>().await;
        });
        let session = start(url);
        events_until(&session, joined);
        let seen = events_until(&session, |e| matches!(e, SessionEvent::Resuming { attempt: 2 }));
        assert!(
            matches!(seen.last(), Some(SessionEvent::Resuming { attempt: 2 })),
            "{seen:?}"
        );
        let quit = Instant::now();
        drop(session);
        assert!(quit.elapsed() < Duration::from_millis(500), "{:?}", quit.elapsed());
    }
}
