//! SHAG `src/engine/network/client/hosts/NetworkGameHost.ts`: the client's
//! side of one game server connection.
//!
//! This is the connection logic without the I/O: the handshake (Join,
//! Welcome, JoinAccept, ResumeToken), snapshot acceptance and acks, clock
//! sync, the stall watchdog, commands and the reliable requests. The owner
//! feeds it what the socket delivers and the time, and sends the [`Outgoing`]
//! messages it returns; [`super::super::network_session`] is that owner for a
//! WebSocket. What the browser host also does (music, the legacy
//! prediction path, interpolation for rendering) belongs to the game and the
//! renderer and is left out.
//!
//! Times: `wall` is `Date.now()` milliseconds (clock sync), `mono` is a
//! monotonic clock in milliseconds (`performance.now()`, the watchdog).

use shagger_shared::net::command::Command;
use shagger_shared::net::game_state::GameState;
use shagger_shared::net::message_codec::{
    self as codec, ClientPrefs, Join, JoinGame, MessageType, ServerMessage, Welcome,
};
use shagger_shared::net::snapshot::EventCodec;

use crate::network::client::clock_sync::ClockSync;
use crate::network::client::connection_watchdog::{ConnectionWatchdog, StallStage};
use crate::network::client::snapshot_receiver::SnapshotReceiver;

/// The tick rate this client simulates at; a server with another is refused.
pub const DEFAULT_TICK_RATE: u8 = 125;
/// How long the handshake may take (ms).
pub const HANDSHAKE_TIMEOUT_MS: f64 = 10_000.0;
/// Spacing of resume attempts after a drop (ms).
pub const RESUME_RETRY_MS: f64 = 2000.0;
/// How often the owner should call [`NetworkGameHost::poll`] (ms).
pub const WATCHDOG_INTERVAL_MS: f64 = 500.0;

/// A message to send. `reliable` is advisory over a WebSocket (lossless).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub data: Vec<u8>,
    pub reliable: bool,
}

impl Outgoing {
    fn reliable(data: Vec<u8>) -> Self {
        Self { data, reliable: true }
    }
    fn unreliable(data: Vec<u8>) -> Self {
        Self { data, reliable: false }
    }
}

/// What the host tells the game.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent<E> {
    /// Welcome from a server with a compatible tick rate (load the map now).
    ServerInfo(Welcome),
    /// The handshake completed: the server's ids for this client.
    Joined {
        player_id: String,
        entity_id: String,
    },
    /// The server's seat for this client survives a drop for `grace_ms`.
    ResumeToken {
        grace_ms: u32,
    },
    /// A pong updated the clock estimate: server − local, and the best round trip (ms).
    ClockSync {
        offset: f64,
        rtt: f64,
    },
    /// A decoded, acknowledged snapshot.
    Snapshot(GameState<E>),
    ServerMessage(ServerMessage),
    /// Snapshots stopped while the socket stays open.
    Stall {
        stage: StallStage,
        silent_ms: f64,
    },
    /// Snapshots came back after a reported stall.
    Recovered {
        stalled_ms: f64,
    },
    /// The connection was refused or failed; the host has stopped.
    Failed(String),
    /// A message could not be handled (it is skipped).
    BadMessage(String),
}

/// Where the connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPhase {
    Idle,
    /// Waiting for Welcome and JoinAccept (and the ResumeToken on a ticketed join).
    Handshake,
    Connected,
    Failed,
}

pub struct NetworkGameHost<C: EventCodec> {
    codec: C,
    tick_rate: u8,
    pub local_player_id: String,
    pub local_entity_id: String,
    pub current_tick: u32,
    phase: HostPhase,
    /// `_isConnected`: JoinAccept received.
    is_connected: bool,
    accepting_messages: bool,
    server_info: Option<Welcome>,
    handshake_deadline: f64,
    wait_for_initial_resume_token: bool,
    resume_token: Option<String>,
    resume_grace_ms: f64,
    client_prefs: Option<ClientPrefs>,
    command_sequence: u32,
    snapshot_receiver: SnapshotReceiver<C::Event>,
    pub clock_sync: ClockSync,
    watchdog: ConnectionWatchdog,
    watchdog_checked_at: f64,
}

impl<C: EventCodec> NetworkGameHost<C> {
    /// `date_to_perf` is `wall - mono` at start, for [`ClockSync::to_local_time`].
    pub fn new(codec: C, date_to_perf: f64) -> Self {
        Self {
            codec,
            tick_rate: DEFAULT_TICK_RATE,
            local_player_id: String::new(),
            local_entity_id: String::new(),
            current_tick: 0,
            phase: HostPhase::Idle,
            is_connected: false,
            accepting_messages: false,
            server_info: None,
            handshake_deadline: 0.0,
            wait_for_initial_resume_token: false,
            resume_token: None,
            resume_grace_ms: 20_000.0,
            client_prefs: None,
            command_sequence: 1,
            snapshot_receiver: SnapshotReceiver::default(),
            clock_sync: ClockSync::new(date_to_perf),
            watchdog: ConnectionWatchdog::default(),
            watchdog_checked_at: 0.0,
        }
    }

    #[must_use]
    pub fn phase(&self) -> HostPhase {
        self.phase
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected
    }

    #[must_use]
    pub fn server_info(&self) -> Option<&Welcome> {
        self.server_info.as_ref()
    }

    /// The seat token and how long the server holds the seat after a drop.
    #[must_use]
    pub fn resume_token(&self) -> Option<(&str, f64)> {
        self.resume_token.as_deref().map(|t| (t, self.resume_grace_ms))
    }

    /// Snapshots dropped because their baseline was gone.
    #[must_use]
    pub fn dropped_snapshots(&self) -> u64 {
        self.snapshot_receiver.dropped_missing_baseline
    }

    /// `connect`, once the transport is open: resets the connection and
    /// returns the Join to send.
    pub fn connect(&mut self, player_name: &str, admission_ticket: Option<&str>, mono: f64) -> Outgoing {
        self.snapshot_receiver.reset();
        self.server_info = None;
        self.is_connected = false;
        self.accepting_messages = true;
        self.resume_token = None;
        self.wait_for_initial_resume_token = admission_ticket.is_some();
        self.phase = HostPhase::Handshake;
        self.handshake_deadline = mono + HANDSHAKE_TIMEOUT_MS;
        Outgoing::reliable(codec::encode_join(&Join {
            player_name: player_name.to_owned(),
            is_bot: false,
            admission_ticket: admission_ticket.map(str::to_owned),
        }))
    }

    /// The reconnect half of `resumeSession`: the transport is open again;
    /// returns the Resume to send. The handshake is complete again with the
    /// next JoinAccept.
    pub fn resume(&mut self, mono: f64) -> Option<Outgoing> {
        let token = self.resume_token.clone()?;
        self.is_connected = false;
        self.server_info = None;
        self.snapshot_receiver.reset();
        self.watchdog.stop();
        self.clock_sync.stop();
        self.accepting_messages = true;
        self.phase = HostPhase::Handshake;
        self.handshake_deadline = mono + HANDSHAKE_TIMEOUT_MS;
        Some(Outgoing::reliable(codec::encode_resume(&token)))
    }

    /// The transport closed (`onDisconnect`). Returns whether the seat can be
    /// resumed: the join had completed and the server sent a token.
    pub fn transport_closed(&mut self) -> bool {
        let was_connected = self.is_connected;
        self.is_connected = false;
        self.accepting_messages = false;
        self.server_info = None;
        self.watchdog.stop();
        self.clock_sync.stop();
        self.snapshot_receiver.reset();
        if self.phase == HostPhase::Handshake {
            self.phase = HostPhase::Failed;
        }
        was_connected && self.resume_token.is_some()
    }

    /// `disconnect`: stop and drop the seat token.
    pub fn disconnect(&mut self) {
        self.accepting_messages = false;
        self.resume_token = None;
        self.wait_for_initial_resume_token = false;
        self.server_info = None;
        self.is_connected = false;
        self.watchdog.stop();
        self.clock_sync.stop();
        self.snapshot_receiver.reset();
        if self.phase != HostPhase::Failed {
            self.phase = HostPhase::Idle;
        }
    }

    fn fail(&mut self, why: String, events: &mut Vec<HostEvent<C::Event>>) {
        self.disconnect();
        self.phase = HostPhase::Failed;
        events.push(HostEvent::Failed(why));
    }

    /// `handleMessage` for one message from the server.
    pub fn handle_message(
        &mut self,
        data: &[u8],
        wall: f64,
        mono: f64,
        out: &mut Vec<Outgoing>,
        events: &mut Vec<HostEvent<C::Event>>,
    ) {
        if !self.accepting_messages {
            return;
        }
        if let Err(why) = self.dispatch(data, wall, mono, out, events) {
            // Keep valid history: an unrelated malformed message must not
            // invalidate acknowledged dependencies.
            events.push(HostEvent::BadMessage(why));
        }
    }

    fn dispatch(
        &mut self,
        data: &[u8],
        wall: f64,
        mono: f64,
        out: &mut Vec<Outgoing>,
        events: &mut Vec<HostEvent<C::Event>>,
    ) -> Result<(), String> {
        let Some(kind) = codec::peek_message_type(data) else {
            return Err("empty message".into());
        };
        match MessageType::from_byte(kind) {
            Some(MessageType::LegacySnapshot | MessageType::Snapshot) => {
                // Snapshots may beat the handshake. Drop them until the timing
                // contract and the local seat are known; never ack them early.
                if self.server_info.is_none() || !self.is_connected {
                    return Ok(());
                }
                // Any snapshot shows the link delivers, even one waiting on a baseline.
                if let Some(stalled_ms) = self.watchdog.data_arrived(mono) {
                    events.push(HostEvent::Recovered { stalled_ms });
                }
                let snapshot = self
                    .snapshot_receiver
                    .receive(data, &self.codec)
                    .map_err(|e| e.to_string())?;
                if let Some(snapshot) = snapshot {
                    self.current_tick = snapshot.tick;
                    // Ack only successfully reconstructed state, at once: ping reads from acks.
                    out.push(Outgoing::reliable(codec::encode_ack(snapshot.tick)));
                    events.push(HostEvent::Snapshot(snapshot));
                }
            }
            Some(MessageType::JoinAccept) => {
                if self.server_info.is_none() {
                    self.fail("Server sent JoinAccept before a compatible Welcome".into(), events);
                    return Ok(());
                }
                let accept = codec::decode_join_accept(data).map_err(|e| e.to_string())?;
                if self.is_connected {
                    return Ok(()); // a duplicate
                }
                self.local_player_id.clone_from(&accept.player_id);
                self.local_entity_id.clone_from(&accept.entity_id);
                self.is_connected = true;
                self.watchdog.start(mono);
                self.watchdog_checked_at = mono;
                if let Some(prefs) = self.client_prefs {
                    out.push(Outgoing::reliable(codec::encode_client_prefs(prefs)));
                }
                if !self.wait_for_initial_resume_token || self.resume_token.is_some() {
                    self.phase = HostPhase::Connected;
                }
                events.push(HostEvent::Joined {
                    player_id: accept.player_id,
                    entity_id: accept.entity_id,
                });
                self.clock_sync.start_sync(wall);
                out.extend(self.clock_sync.poll(wall).into_iter().map(Outgoing::unreliable));
            }
            Some(MessageType::ResumeToken) => {
                let token = codec::decode_resume_token(data).map_err(|e| e.to_string())?;
                self.resume_token = Some(token.token);
                self.resume_grace_ms = f64::from(token.grace_ms);
                if self.is_connected && self.wait_for_initial_resume_token && self.phase == HostPhase::Handshake {
                    self.wait_for_initial_resume_token = false;
                    self.phase = HostPhase::Connected;
                }
                events.push(HostEvent::ResumeToken {
                    grace_ms: token.grace_ms,
                });
            }
            Some(MessageType::Welcome) => {
                let welcome = codec::decode_welcome(data).map_err(|e| e.to_string())?;
                if welcome.tick_rate != self.tick_rate {
                    let why = format!(
                        "Unsupported server tick rate: {}Hz; this client requires {}Hz.",
                        welcome.tick_rate, self.tick_rate
                    );
                    self.fail(why, events);
                    return Ok(());
                }
                self.server_info = Some(welcome.clone());
                events.push(HostEvent::ServerInfo(welcome));
            }
            Some(MessageType::Pong) => {
                let pong = codec::decode_pong(data).map_err(|e| e.to_string())?;
                self.clock_sync
                    .process_pong(pong.client_send_time, pong.server_time, wall);
                if self.clock_sync.is_synced() {
                    events.push(HostEvent::ClockSync {
                        offset: self.clock_sync.offset(),
                        rtt: self.clock_sync.rtt(),
                    });
                }
            }
            Some(MessageType::ServerMessage) => {
                let message = codec::decode_server_message(data).map_err(|e| e.to_string())?;
                events.push(HostEvent::ServerMessage(message));
            }
            _ => return Err(format!("Unknown message type: 0x{kind:x}")),
        }
        Ok(())
    }

    /// Timers: clock-sync pings, the handshake timeout and the watchdog
    /// (`checkWatchdog`). Call about every [`WATCHDOG_INTERVAL_MS`].
    pub fn poll(&mut self, wall: f64, mono: f64, out: &mut Vec<Outgoing>, events: &mut Vec<HostEvent<C::Event>>) {
        // A ticketed join is not done until its resume token arrives
        // (`_onJoinAccept` waits for it, and so does the connect timer).
        let waiting = !self.is_connected || self.wait_for_initial_resume_token;
        if self.phase == HostPhase::Handshake && waiting && mono >= self.handshake_deadline {
            self.fail("Timeout waiting for Welcome and JoinAccept".into(), events);
            return;
        }
        out.extend(self.clock_sync.poll(wall).into_iter().map(Outgoing::unreliable));
        if !self.is_connected {
            return;
        }
        let since_last_check = mono - self.watchdog_checked_at;
        self.watchdog_checked_at = mono;
        // A blocked loop leaves snapshots queued unread: that gap says nothing
        // about the connection.
        if since_last_check > WATCHDOG_INTERVAL_MS * 3.0 {
            self.watchdog.skip_gap(mono);
            return;
        }
        if let Some(stage) = self.watchdog.check(mono) {
            events.push(HostEvent::Stall {
                stage,
                silent_ms: self.watchdog.silent_for(mono),
            });
        }
    }

    /// `sendCommand`: numbers the command and encodes it (unreliable).
    ///
    /// Every command the game hands over takes a number, even one dropped
    /// while the link is down: the game predicted it on another thread, and
    /// the acks must keep naming the commands it numbered the same way.
    pub fn send_command(&mut self, command: &Command) -> Option<Outgoing> {
        let seq = self.command_sequence;
        self.command_sequence += 1;
        if !self.is_connected {
            return None;
        }
        Some(Outgoing::unreliable(codec::encode_command(
            &self.local_player_id,
            seq,
            command,
        )))
    }

    /// `sendJoinGame`: spectator to player.
    pub fn send_join_game(
        &self,
        team_id: Option<&str>,
        manual_pick: bool,
        admission_ticket: Option<&str>,
    ) -> Option<Outgoing> {
        self.is_connected.then(|| {
            Outgoing::reliable(codec::encode_join_game(&JoinGame {
                team_id: team_id.map(str::to_owned),
                manual_pick,
                admission_ticket: admission_ticket.map(str::to_owned),
            }))
        })
    }

    /// `sendSpectate`: player to spectator.
    #[must_use]
    pub fn send_spectate(&self) -> Option<Outgoing> {
        self.is_connected.then(|| Outgoing::reliable(codec::encode_spectate()))
    }

    /// `sendChat`: trimmed, cut at 128 UTF-16 units, `/say_team ` for team chat.
    #[must_use]
    pub fn send_chat(&self, text: &str, team_only: bool) -> Option<Outgoing> {
        if !self.is_connected {
            return None;
        }
        let trimmed = shagger_shared::js::js_trim(text);
        if trimmed.is_empty() {
            return None;
        }
        let units: Vec<u16> = trimmed.encode_utf16().take(128).collect();
        let cut = String::from_utf16_lossy(&units);
        let prefix = if team_only { "/say_team " } else { "" };
        Some(Outgoing::reliable(codec::encode_chat(&format!("{prefix}{cut}"))))
    }

    /// `setClientPrefs`: kept and sent again after every JoinAccept.
    pub fn set_client_prefs(&mut self, prefs: ClientPrefs) -> Option<Outgoing> {
        self.client_prefs = Some(prefs);
        self.is_connected
            .then(|| Outgoing::reliable(codec::encode_client_prefs(prefs)))
    }

    /// `Game.sendRequestSpawn`: back into play after a death.
    #[must_use]
    pub fn send_request_spawn(&self) -> Option<Outgoing> {
        self.is_connected
            .then(|| Outgoing::reliable(codec::encode_request_spawn()))
    }

    /// `sendLoadout`.
    #[must_use]
    pub fn send_loadout(&self, primary: &str, secondary: &str) -> Option<Outgoing> {
        self.is_connected
            .then(|| Outgoing::reliable(codec::encode_set_loadout(primary, secondary)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger_shared::net::binary_protocol::BinaryEncoder;
    use shagger_shared::net::message_codec::{
        JoinAccept, ResumeToken, decode_join, encode_join_accept, encode_resume_token, encode_welcome,
    };
    use shagger_shared::net::snapshot::NoEvents;

    type Host = NetworkGameHost<NoEvents>;
    type Events = Vec<HostEvent<std::convert::Infallible>>;

    fn welcome(tick_rate: u8) -> Vec<u8> {
        encode_welcome(&Welcome {
            server_id: "s".into(),
            server_name: "Test".into(),
            map: "washoi_dm2".into(),
            tick_rate,
            max_players: 16,
            current_players: 1,
        })
    }

    fn accept() -> Vec<u8> {
        encode_join_accept(&JoinAccept {
            client_id: "c1".into(),
            player_id: "player_1".into(),
            entity_id: "player_1".into(),
        })
    }

    /// A full snapshot with no entities, players or events.
    fn snapshot(tick: u32) -> Vec<u8> {
        let mut e = BinaryEncoder::new();
        e.write_uint8(MessageType::Snapshot as u8);
        e.write_uint8(0);
        e.write_uint32(0);
        e.write_uint32(tick);
        e.write_float64(1.0);
        e.write_uint32(0);
        e.write_uint16(0);
        e.write_uint16(0);
        e.write_uint16(0);
        e.into_data()
    }

    fn feed(host: &mut Host, data: &[u8], mono: f64) -> (Vec<Outgoing>, Events) {
        let (mut out, mut events) = (Vec::new(), Vec::new());
        host.handle_message(data, 1000.0 + mono, mono, &mut out, &mut events);
        (out, events)
    }

    fn joined_host() -> Host {
        let mut host = Host::new(NoEvents, 0.0);
        let join = host.connect("Rusty", None, 0.0);
        let decoded = decode_join(&join.data).unwrap();
        assert_eq!(decoded.player_name, "Rusty");
        feed(&mut host, &welcome(125), 1.0);
        let (out, events) = feed(&mut host, &accept(), 2.0);
        assert_eq!(host.phase(), HostPhase::Connected);
        assert!(matches!(&events[0], HostEvent::Joined { player_id, .. } if player_id == "player_1"));
        // The first clock-sync ping goes out with the accept.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data[0], MessageType::Ping as u8);
        host
    }

    #[test]
    fn handshake_then_snapshots_are_acked_in_order() {
        let mut host = joined_host();
        let (out, events) = feed(&mut host, &snapshot(10), 3.0);
        assert_eq!(out, vec![Outgoing::reliable(codec::encode_ack(10))]);
        assert!(matches!(&events[0], HostEvent::Snapshot(s) if s.tick == 10));
        // Stale and repeated ticks are dropped without an ack.
        let (out, events) = feed(&mut host, &snapshot(10), 4.0);
        assert!(out.is_empty() && events.is_empty());
        assert_eq!(host.current_tick, 10);
    }

    #[test]
    fn snapshots_before_the_join_are_dropped() {
        let mut host = Host::new(NoEvents, 0.0);
        host.connect("Rusty", None, 0.0);
        let (out, events) = feed(&mut host, &snapshot(5), 1.0);
        assert!(out.is_empty() && events.is_empty());
        feed(&mut host, &welcome(125), 1.0);
        let (out, _) = feed(&mut host, &snapshot(6), 1.0);
        assert!(out.is_empty());
    }

    #[test]
    fn refusals_fail_the_connection() {
        let mut host = Host::new(NoEvents, 0.0);
        host.connect("Rusty", None, 0.0);
        let (_, events) = feed(&mut host, &welcome(60), 1.0);
        assert!(matches!(&events[0], HostEvent::Failed(why) if why.contains("60Hz")));
        assert_eq!(host.phase(), HostPhase::Failed);

        let mut host = Host::new(NoEvents, 0.0);
        host.connect("Rusty", None, 0.0);
        let (_, events) = feed(&mut host, &accept(), 1.0);
        assert!(matches!(&events[0], HostEvent::Failed(_)));

        let mut host = Host::new(NoEvents, 0.0);
        host.connect("Rusty", None, 0.0);
        let (mut out, mut events) = (Vec::new(), Vec::new());
        host.poll(0.0, HANDSHAKE_TIMEOUT_MS - 1.0, &mut out, &mut events);
        assert!(events.is_empty());
        host.poll(0.0, HANDSHAKE_TIMEOUT_MS, &mut out, &mut events);
        assert!(matches!(&events[0], HostEvent::Failed(why) if why.starts_with("Timeout")));
    }

    #[test]
    fn a_ticketed_join_waits_for_the_resume_token() {
        let mut host = Host::new(NoEvents, 0.0);
        host.connect("Rusty", Some("ticket"), 0.0);
        feed(&mut host, &welcome(125), 1.0);
        feed(&mut host, &accept(), 2.0);
        assert!(host.is_connected());
        assert_eq!(host.phase(), HostPhase::Handshake);
        feed(
            &mut host,
            &encode_resume_token(&ResumeToken {
                token: "tok".into(),
                grace_ms: 15_000,
            }),
            3.0,
        );
        assert_eq!(host.phase(), HostPhase::Connected);
        assert_eq!(host.resume_token(), Some(("tok", 15_000.0)));
        // A drop now can be resumed, with the token.
        assert!(host.transport_closed());
        let resume = host.resume(10.0).unwrap();
        assert_eq!(codec::decode_resume(&resume.data).unwrap(), "tok");
    }

    #[test]
    fn a_ticketed_join_without_its_token_times_out() {
        let mut host = Host::new(NoEvents, 0.0);
        host.connect("Rusty", Some("ticket"), 0.0);
        feed(&mut host, &welcome(125), 1.0);
        feed(&mut host, &accept(), 2.0);
        let (mut out, mut events) = (Vec::new(), Vec::new());
        host.poll(0.0, HANDSHAKE_TIMEOUT_MS, &mut out, &mut events);
        assert!(events.iter().any(|e| matches!(e, HostEvent::Failed(_))));
        assert_eq!(host.phase(), HostPhase::Failed);
    }

    #[test]
    fn commands_take_numbers_while_the_link_is_down() {
        let mut host = joined_host();
        assert!(host.send_command(&Command::default()).is_some());
        host.transport_closed();
        assert!(host.send_command(&Command::default()).is_none());
        let next = host.send_command(&Command::default());
        assert!(next.is_none());
        assert_eq!(host.command_sequence, 4);
    }

    #[test]
    fn the_watchdog_reports_a_stall_and_skips_gaps_the_client_caused() {
        let mut host = joined_host();
        let (mut out, mut events) = (Vec::new(), Vec::new());
        let mut t = 2.0;
        while t < 2.0 + 2600.0 {
            t += 500.0;
            host.poll(1000.0 + t, t, &mut out, &mut events);
        }
        assert!(events.iter().any(|e| matches!(
            e,
            HostEvent::Stall {
                stage: StallStage::Warn,
                ..
            }
        )));
        // A long pause in polling is the client's own gap.
        let mut host = joined_host();
        events.clear();
        host.poll(0.0, 5000.0, &mut out, &mut events);
        host.poll(0.0, 5500.0, &mut out, &mut events);
        assert!(!events.iter().any(|e| matches!(e, HostEvent::Stall { .. })));
    }

    #[test]
    fn requests_need_a_connection_and_chat_is_cut_like_the_browser() {
        let mut host = Host::new(NoEvents, 0.0);
        assert!(host.send_spectate().is_none());
        assert!(host.set_client_prefs(ClientPrefs { auto_switch: false }).is_none());
        let mut host = joined_host();
        assert!(host.send_chat("   ", false).is_none());
        let long = "é".repeat(200);
        let chat = host.send_chat(&format!("  {long} "), true).unwrap();
        let text = codec::decode_chat(&chat.data).unwrap();
        assert_eq!(text, format!("/say_team {}", "é".repeat(128)));
        let a = host.send_command(&Command::default()).unwrap();
        let b = host.send_command(&Command::default()).unwrap();
        assert_eq!(codec::peek_command_seq(&a.data), Some(1));
        assert_eq!(codec::peek_command_seq(&b.data), Some(2));
    }
}
