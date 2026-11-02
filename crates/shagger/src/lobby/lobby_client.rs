//! SHAG `src/lobby/LobbyClient.ts`: the client's lobby connection.
//!
//! Like [`crate::network::client::network_session`], the WebSocket runs on a
//! network thread of its own and talks to the game through channels: the
//! request methods queue a JSON message, and [`LobbyClient::poll_events`]
//! returns what arrived. Messages are sent as `JSON.stringify` would, with
//! absent optional fields left out.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc as async_mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use super::types::LobbyIn;

/// How long to wait for the connection before giving up.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// The close reason when this client closes the connection itself.
pub const CLIENT_DISCONNECT_REASON: &str = "Client disconnect";

/// What the lobby connection reports.
#[derive(Debug, Clone, PartialEq)]
pub enum LobbyEvent {
    /// `_connected`.
    Connected,
    Message(LobbyIn),
    /// `_disconnected`; nothing more arrives.
    Disconnected {
        reason: String,
        code: u16,
    },
    /// `_error`: the connection failed.
    Error(String),
}

enum Outgoing {
    Text(String),
    Close,
}

pub struct LobbyClient {
    requests: async_mpsc::UnboundedSender<Outgoing>,
    events: mpsc::Receiver<LobbyEvent>,
    thread: Option<thread::JoinHandle<()>>,
    request_serial: u64,
}

/// A message object without the fields whose value is `None`.
fn message(kind: &str, fields: &[(&str, Option<Value>)]) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), Value::String(kind.into()));
    for (k, v) in fields {
        if let Some(v) = v {
            m.insert((*k).into(), v.clone());
        }
    }
    Value::Object(m)
}

fn s(x: &str) -> Option<Value> {
    Some(Value::String(x.into()))
}

impl LobbyClient {
    /// Opens the connection on a new thread. [`LobbyEvent::Connected`] or
    /// [`LobbyEvent::Error`] follows.
    ///
    /// # Panics
    /// When the thread or its runtime cannot be created.
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (request_tx, request_rx) = async_mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel();
        let url = url.to_owned();
        let thread = thread::Builder::new()
            .name("shagger-lobby".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("lobby runtime");
                runtime.block_on(run(&url, request_rx, &event_tx));
            })
            .expect("lobby thread");
        Self {
            requests: request_tx,
            events: event_rx,
            thread: Some(thread),
            request_serial: 0,
        }
    }

    /// Everything that arrived since the last call.
    pub fn poll_events(&self) -> Vec<LobbyEvent> {
        self.events.try_iter().collect()
    }

    /// Waits up to `timeout` for the next event.
    pub fn next_event(&self, timeout: Duration) -> Option<LobbyEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Sends any message object as it is.
    pub fn send(&self, message: &Value) {
        let _ = self.requests.send(Outgoing::Text(message.to_string()));
    }

    /// Closes the connection; `Disconnected` follows.
    pub fn disconnect(&self) {
        let _ = self.requests.send(Outgoing::Close);
    }

    // ── Auth ──

    /// Log in with the player's key, or without one to get a new account
    /// (its key arrives once, in `welcome`).
    pub fn auth(&self, key: Option<&str>) {
        self.send(&message("auth", &[("key", key.and_then(s))]));
    }

    pub fn rename(&self, name: &str) {
        self.send(&message("identity:rename", &[("name", s(name))]));
    }

    // ── Chat ──

    pub fn chat_send(&self, text: &str) {
        self.send(&message("chat:send", &[("text", s(text))]));
    }

    pub fn room_chat_send(&self, lobby_id: &str, text: &str) {
        self.send(&message(
            "lobby:chat_send",
            &[("lobbyId", s(lobby_id)), ("text", s(text))],
        ));
    }

    // ── Rooms ──

    pub fn create_lobby(&self, gametype: &str, private: bool) {
        let visibility = if private { "private" } else { "public" };
        self.send(&message(
            "lobby:create",
            &[("gametype", s(gametype)), ("visibility", s(visibility))],
        ));
    }

    pub fn join_lobby(&self, lobby_id: &str, invite_token: Option<&str>) {
        self.send(&message(
            "lobby:join",
            &[("lobbyId", s(lobby_id)), ("inviteToken", invite_token.and_then(s))],
        ));
    }

    pub fn leave_lobby(&self, lobby_id: &str) {
        self.send(&message("lobby:leave", &[("lobbyId", s(lobby_id))]));
    }

    pub fn vote_map(&self, lobby_id: &str, map: &str) {
        self.send(&message("lobby:vote_map", &[("lobbyId", s(lobby_id)), ("map", s(map))]));
    }

    pub fn set_ready(&self, lobby_id: &str, ready: bool) {
        self.send(&message(
            "lobby:ready",
            &[("lobbyId", s(lobby_id)), ("ready", Some(Value::Bool(ready)))],
        ));
    }

    pub fn vote_shuffle(&self, lobby_id: &str) {
        self.send(&message("lobby:vote_shuffle", &[("lobbyId", s(lobby_id))]));
    }

    pub fn switch_team(&self, lobby_id: &str) {
        self.send(&message("lobby:switch_team", &[("lobbyId", s(lobby_id))]));
    }

    pub fn invite_bot(&self, lobby_id: &str, difficulty: Option<&str>) {
        self.send(&message(
            "lobby:invite_bot",
            &[("lobbyId", s(lobby_id)), ("difficulty", difficulty.and_then(s))],
        ));
    }

    // ── Queue ──

    pub fn join_queue(&self, gametype: &str) {
        self.send(&message("queue:join", &[("gametype", s(gametype))]));
    }

    pub fn leave_queue(&self) {
        self.send(&message("queue:leave", &[]));
    }

    // ── Requests with an answer ──

    fn next_request_id(&mut self, prefix: &str) -> String {
        self.request_serial += 1;
        format!("{prefix}-{}", self.request_serial)
    }

    /// `requestDemoList`; the answer is `demos:list_response` with this id.
    pub fn request_demo_list(&mut self, limit: u32, yours: bool) -> String {
        let request_id = self.next_request_id("demos");
        let scope = if yours { "yours" } else { "latest" };
        self.send(&json!({ "type": "demos:list", "limit": limit, "scope": scope, "requestId": request_id }));
        request_id
    }

    /// `requestSettings`: the answer is `settings:data` with this id.
    pub fn request_settings(&mut self) -> String {
        let request_id = self.next_request_id("settings");
        self.send(&json!({ "type": "settings:get", "requestId": request_id }));
        request_id
    }

    /// `saveSettings`: the answer is `settings:saved` or `settings:error`.
    pub fn save_settings(&mut self, settings: &str) -> String {
        let request_id = self.next_request_id("settings");
        self.send(&json!({ "type": "settings:save", "requestId": request_id, "settings": settings }));
        request_id
    }

    /// `leaderboard:get` (TrueSkill); the answer is `leaderboard:page` or
    /// `leaderboard:error` with this id.
    pub fn request_leaderboard(
        &mut self,
        gametype: &str,
        period: &str,
        offset: u32,
        limit: u32,
        private: bool,
    ) -> String {
        let request_id = self.next_request_id("leaderboard");
        let scope = if private { "private" } else { "public" };
        self.send(&json!({
            "type": "leaderboard:get", "requestId": request_id, "gametype": gametype, "period": period,
            "offset": offset, "limit": limit, "scope": scope, "system": "trueskill"
        }));
        request_id
    }

    /// `stats:get`: this account's summary (`stats:summary`).
    pub fn request_stats(&mut self, gametype: &str, period: &str, private: bool) -> String {
        let request_id = self.next_request_id("stats");
        let scope = if private { "private" } else { "public" };
        self.send(&json!({
            "type": "stats:get", "requestId": request_id, "gametype": gametype, "scope": scope, "period": period
        }));
        request_id
    }

    /// `match:seat_reissue`: a new ticket when the one from `lobby:connect` expired.
    pub fn request_seat_reissue(&mut self, match_id: &str) -> String {
        let request_id = self.next_request_id("seat");
        self.send(&json!({ "type": "match:seat_reissue", "requestId": request_id, "matchId": match_id }));
        request_id
    }
}

impl Drop for LobbyClient {
    fn drop(&mut self) {
        let _ = self.requests.send(Outgoing::Close);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn run(url: &str, mut requests: async_mpsc::UnboundedReceiver<Outgoing>, events: &mpsc::Sender<LobbyEvent>) {
    let connect = tokio_tungstenite::connect_async_tls_with_config(
        url,
        None,
        true,
        Some(crate::network::client::tls::connector()),
    );
    let mut socket = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(Ok((socket, _))) => socket,
        Ok(Err(e)) => {
            let _ = events.send(LobbyEvent::Error(format!("WebSocket connection error: {e}")));
            return;
        }
        Err(_) => {
            let secs = CONNECT_TIMEOUT.as_secs();
            let _ = events.send(LobbyEvent::Error(format!("Lobby server did not answer in {secs}s")));
            return;
        }
    };
    let _ = events.send(LobbyEvent::Connected);
    let (reason, code) = loop {
        tokio::select! {
            incoming = socket.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    // The browser logs and drops what is not a typed message.
                    if let Some(m) = LobbyIn::parse(text.as_str()) {
                        let _ = events.send(LobbyEvent::Message(m));
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    break frame.map_or_else(|| (String::new(), 1005), |f| (f.reason.to_string(), u16::from(f.code)));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => break (e.to_string(), 1006),
                None => break (String::new(), 1006),
            },
            request = requests.recv() => match request {
                Some(Outgoing::Text(text)) => {
                    if let Err(e) = socket.send(Message::text(text)).await {
                        break (e.to_string(), 1006);
                    }
                }
                None | Some(Outgoing::Close) => {
                    let _ = socket.close(Some(CloseFrame { code: CloseCode::Normal, reason: CLIENT_DISCONNECT_REASON.into() })).await;
                    break (CLIENT_DISCONNECT_REASON.to_owned(), 1000);
                }
            },
        }
    };
    let _ = events.send(LobbyEvent::Disconnected { reason, code });
}
