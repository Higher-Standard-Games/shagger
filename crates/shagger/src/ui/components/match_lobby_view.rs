//! SHAG `src/engine/ui/components/MatchLobbyView.ts` and
//! `styles/MatchLobbyView.css`: one room of the lobby before its match.
//! Which room it is and its invite; a status slab that says what the room
//! waits on; the seats; the map vote; and the keys to leave, add a bot,
//! switch or shuffle sides, rejoin a running match and ready up. Quick
//! play's self-driving fill (`fillWithBots`) is here too.
//!
//! The browser builds this from HTML and CSS; here the same state paints
//! with egui, in the stage's accent ([`RoomStyle`]). The lobby's messages
//! come in through [`MatchLobbyView::handle`], and what the player does goes
//! straight to the [`LobbyClient`], except what the panel holding the room
//! must do itself ([`RoomAction`]).
//!
//! Engine code: the game names its fonts, its site and its levelshots.

use std::time::{Duration, Instant};

use egui::epaint::Vertex;
use egui::{Color32, FontFamily, Mesh, Painter, Pos2, Rect, Shape, Stroke, TextureId, pos2, vec2};
use serde_json::Value;

use super::lobby_twin::{clamp_css, cream, elide, gradient_polygon, gradient_polygon_x, hsl, hsla, text};
use super::raised_key::{Accent, KeyFace, KeyLook, KeyText, KeyTextLayout, RaisedKey};
use crate::lobby::lobby_client::LobbyClient;
use crate::lobby::types::LobbyIn;

mod match_lobby_draw;
pub use match_lobby_draw::invite_href;

/// The bot skill presets the lobby accepts (`lobby:invite_bot`), easiest
/// first: the id, and its name in the menu.
const BOT_LEVELS: [(&str, &str); 4] = [
    ("easy", "Easy"),
    ("medium", "Medium"),
    ("hard", "Hard"),
    ("insane", "Insane"),
];

/// How long an unanswered bot invite holds the key down.
const BOT_INVITE_TIMEOUT: Duration = Duration::from_secs(9);
/// How long a flash note and a key's changed hint stay.
const FLASH: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub struct MatchLobbyPlayer {
    pub id: String,
    pub name: String,
    /// `alpha`, `bravo` or `ffa`.
    pub team: String,
    pub ready: bool,
    pub is_local: bool,
    pub bot_level: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchLobbyMapOption {
    pub name: String,
    pub display_name: String,
    pub votes: u32,
    pub voted_by_local: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Starting,
}

/// What the room asks of the panel holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomAction {
    Leave,
    /// `recoverSeat`: a new ticket for this match's seat. The panel asks the
    /// lobby and answers with [`MatchLobbyView::seat_unavailable`] on a refusal.
    Rejoin(String),
}

/// What the room is drawn with: the stage's accent and the game's faces.
pub struct RoomStyle<'a> {
    pub accent: Accent,
    pub display: FontFamily,
    pub body: FontFamily,
    /// The window's width, for the CSS's `vw`.
    pub window_width: f32,
    /// `max-height: 820px`: spacing and sizes give way.
    pub short: bool,
    /// `max-width: 1500px`.
    pub narrow: bool,
    /// `max-width: 1100px`: two seats and two maps a row.
    pub narrower: bool,
    /// The site an invite opens, such as `https://eu.washoi.net`.
    pub site: &'a str,
    /// The lobby connection is up: Rejoin needs it.
    pub online: bool,
    /// A map's levelshot (`/data/levelshots/<map>.jpg`), once it is loaded.
    pub map_shot: &'a dyn Fn(&str) -> Option<TextureId>,
}

#[derive(Debug)]
pub struct MatchLobbyView {
    pub lobby_id: String,
    pub gametype: String,
    pub gametype_label: String,
    pub max_players: usize,
    pub min_players: usize,
    pub has_teams: bool,
    pub team_names: (String, String),
    pub private_lobby: bool,
    /// The link that brings someone into this room; a private room's carries its invite.
    pub share_url: String,
    pub maps: Vec<MatchLobbyMapOption>,
    pub players: Vec<MatchLobbyPlayer>,
    pub local_ready: bool,
    pub phase: Phase,
    pub match_in_progress: bool,
    pub room_error: Option<String>,
    pub active_match_id: Option<String>,
    local_player_id: Option<String>,
    invite_bot_pending: Option<Instant>,
    /// Quick play: bots still to add; 0 when off. The player readies up themselves.
    auto_fill_bots: usize,
    /// The skill menu beside Add bot is open.
    bot_menu: bool,
    /// A note beside the Add bot key, such as "Lobby changed to private".
    flash: Option<(String, Instant)>,
    /// A key's hint changed for a moment: Add bot's "No bots available",
    /// Copy's "Copied", Shuffle's "Vote sent", Rejoin's "Checking seat…".
    bot_note: Option<(String, Instant)>,
    copied: Option<Instant>,
    shuffle_sent: bool,
    recover_note: Option<String>,
    opened_at: Instant,
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn player_of(p: &Value, local: Option<&str>) -> MatchLobbyPlayer {
    let id = str_of(p, "id").to_owned();
    MatchLobbyPlayer {
        is_local: local == Some(id.as_str()),
        id,
        name: str_of(p, "name").to_owned(),
        team: str_of(p, "team").to_owned(),
        ready: p.get("ready").and_then(Value::as_bool).unwrap_or(false),
        bot_level: p.get("botLevel").and_then(Value::as_str).map(str::to_owned),
    }
}

/// Upper-cases the first letter, as `botLevel[0].toUpperCase()` does.
fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

impl MatchLobbyView {
    /// `new MatchLobbyView(gametype, lobbyClient, lobbyInfo, localPlayerId,
    /// { fillWithBots })`. `seed` picks quick play's map.
    #[must_use]
    pub fn new(
        lobby: &Value,
        local_player_id: Option<&str>,
        fill_with_bots: usize,
        seed: u64,
        client: &LobbyClient,
    ) -> Self {
        let gametype = str_of(lobby, "gametype").to_owned();
        let config = lobby.get("config").cloned().unwrap_or(Value::Null);
        let max_players = config.get("maxPlayers").and_then(Value::as_u64).unwrap_or(2) as usize;
        let min_players = config
            .get("minPlayers")
            .and_then(Value::as_u64)
            .map_or(max_players, |m| m as usize)
            .min(max_players);
        let team_names = config.get("teamNames");
        let team = |key: &str, fallback: &str| {
            team_names
                .and_then(|t| t.get(key))
                .and_then(Value::as_str)
                .unwrap_or(fallback)
                .to_owned()
        };
        let gametype_label = match gametype.as_str() {
            "duel" => "DUEL 1v1".to_owned(),
            "snd" => "S&D".to_owned(),
            g => g.to_uppercase(),
        };
        let id = str_of(lobby, "id").to_owned();
        let private_lobby = str_of(lobby, "visibility") == "private";
        // Only private rooms need the server's bearer-token link.
        let share_url = if private_lobby {
            str_of(lobby, "shareUrl").to_owned()
        } else {
            format!("/?lobby={id}")
        };
        let active_match_id = lobby.get("activeMatchId").and_then(Value::as_str).map(str::to_owned);
        let maps = lobby
            .get("maps")
            .and_then(Value::as_array)
            .map(|maps| {
                maps.iter()
                    .map(|m| MatchLobbyMapOption {
                        name: str_of(m, "name").to_owned(),
                        display_name: str_of(m, "displayName").to_owned(),
                        votes: m.get("votes").and_then(Value::as_u64).unwrap_or(0) as u32,
                        voted_by_local: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let players: Vec<MatchLobbyPlayer> = lobby
            .get("players")
            .and_then(Value::as_array)
            .map(|ps| ps.iter().map(|p| player_of(p, local_player_id)).collect())
            .unwrap_or_default();
        let mut view = Self {
            lobby_id: id,
            gametype_label,
            max_players,
            min_players,
            has_teams: config.get("hasTeams").and_then(Value::as_bool).unwrap_or(false),
            team_names: (team("alpha", "Team Soba"), team("bravo", "Team Udon")),
            private_lobby,
            share_url,
            local_ready: players.iter().find(|p| p.is_local).is_some_and(|p| p.ready),
            maps,
            players,
            phase: if active_match_id.is_some() {
                Phase::Starting
            } else {
                Phase::Waiting
            },
            match_in_progress: active_match_id.is_some(),
            room_error: None,
            active_match_id,
            local_player_id: local_player_id.map(str::to_owned),
            invite_bot_pending: None,
            bot_menu: false,
            auto_fill_bots: 0,
            flash: None,
            bot_note: None,
            copied: None,
            shuffle_sent: false,
            recover_note: None,
            opened_at: Instant::now(),
            gametype,
        };
        if fill_with_bots > 0 && view.active_match_id.is_none() {
            view.auto_fill_bots = fill_with_bots;
            let offered: Vec<String> = view
                .maps
                .iter()
                .filter(|m| !m.name.is_empty())
                .map(|m| m.name.clone())
                .collect();
            if !offered.is_empty() {
                let pick = offered[(seed % offered.len() as u64) as usize].clone();
                view.select_map_vote(&pick, client);
            }
            view.continue_auto_fill(client);
        }
        view
    }

    /// The lobby's messages for this room.
    pub fn handle(&mut self, message: &LobbyIn, client: &LobbyClient) {
        match message {
            LobbyIn::Other { kind, message } => self.handle_other(kind, message, client),
            LobbyIn::LobbyError(error) => {
                self.room_error = Some(error.clone());
                self.phase = if self.active_match_id.is_some() {
                    Phase::Starting
                } else {
                    Phase::Waiting
                };
            }
            LobbyIn::Connect(connect) => {
                self.room_error = None;
                self.active_match_id.clone_from(&connect.match_id);
                self.phase = Phase::Starting;
            }
            _ => {}
        }
    }

    fn handle_other(&mut self, kind: &str, msg: &Value, client: &LobbyClient) {
        match kind {
            "lobby:votes_updated" => {
                if let Some(votes) = msg.get("votes") {
                    for map in &mut self.maps {
                        if let Some(n) = votes.get(&map.name).and_then(Value::as_u64) {
                            map.votes = n as u32;
                        }
                    }
                }
            }
            "lobby:player_updated" => {
                let id = str_of(msg, "playerId");
                let ready = msg.get("ready").and_then(Value::as_bool).unwrap_or(false);
                if let Some(player) = self.players.iter_mut().find(|p| p.id == id) {
                    player.ready = ready;
                    if player.is_local {
                        self.local_ready = ready;
                    }
                }
            }
            "lobby:player_joined" => {
                let Some(p) = msg.get("player") else { return };
                let arrival = player_of(p, self.local_player_id.as_deref());
                // One seat per id: a join for someone already seated replaces them.
                match self.players.iter().position(|p| p.id == arrival.id) {
                    Some(i) => self.players[i] = arrival,
                    None => self.players.push(arrival),
                }
                self.invite_bot_pending = None;
                self.continue_auto_fill(client);
            }
            "lobby:player_left" => {
                let id = str_of(msg, "playerId");
                self.players.retain(|p| p.id != id);
            }
            "lobby:made_private" => {
                if str_of(msg, "lobbyId") == self.lobby_id {
                    self.private_lobby = true;
                    str_of(msg, "shareUrl").clone_into(&mut self.share_url);
                    self.flash = Some(("Lobby changed to private".into(), Instant::now()));
                }
            }
            "lobby:bot_invite_failed" => {
                self.invite_bot_pending = None;
                self.auto_fill_bots = 0;
                self.bot_note = Some(("No bots available".into(), Instant::now()));
            }
            "lobby:all_ready" => {
                self.room_error = None;
                self.phase = Phase::Starting;
                for p in &mut self.players {
                    p.ready = true;
                }
            }
            "lobby:teams_shuffled" => {
                // By id: names are display text and can collide between bots.
                for sp in msg.get("players").and_then(Value::as_array).into_iter().flatten() {
                    let id = str_of(sp, "id");
                    if let Some(p) = self.players.iter_mut().find(|p| p.id == id) {
                        str_of(sp, "team").clone_into(&mut p.team);
                    }
                }
                self.shuffle_sent = false;
            }
            "match:ended" => {
                self.room_error = None;
                self.active_match_id = None;
                self.match_in_progress = false;
                self.recover_note = None;
                self.phase = Phase::Waiting;
                self.local_ready = false;
                for p in &mut self.players {
                    p.ready = false;
                }
                for map in &mut self.maps {
                    map.votes = 0;
                    map.voted_by_local = false;
                }
            }
            _ => {}
        }
    }

    /// The lobby had no seat to give back (`match:seat_reissue_error`).
    pub fn seat_unavailable(&mut self) {
        self.recover_note = Some("No seat to return to".into());
    }

    /// Per frame: an invite the server never answered lets go.
    pub fn tick(&mut self) {
        if self
            .invite_bot_pending
            .is_some_and(|at| at.elapsed() >= BOT_INVITE_TIMEOUT)
        {
            self.invite_bot_pending = None;
            self.auto_fill_bots = 0;
        }
        if self.flash.as_ref().is_some_and(|(_, at)| at.elapsed() >= FLASH) {
            self.flash = None;
        }
        if self.bot_note.as_ref().is_some_and(|(_, at)| at.elapsed() >= FLASH) {
            self.bot_note = None;
        }
        if self.copied.is_some_and(|at| at.elapsed() >= FLASH) {
            self.copied = None;
        }
    }

    pub fn bot_count(&self) -> usize {
        self.players.iter().filter(|p| p.bot_level.is_some()).count()
    }

    fn local_team(&self) -> Option<&str> {
        self.players.iter().find(|p| p.is_local).map(|p| p.team.as_str())
    }

    /// `teamName`: what this room calls a side.
    #[must_use]
    pub fn team_name(&self, team: &str) -> &str {
        if team == "alpha" {
            &self.team_names.0
        } else {
            &self.team_names.1
        }
    }

    /// `shape`: "1 vs 1", "Up to 5 vs 5 · starts at 2", "Free for all · 8 players".
    #[must_use]
    pub fn shape(&self) -> String {
        let (max, min) = (self.max_players, self.min_players);
        if max == 2 {
            return "1 vs 1".into();
        }
        let starts = if min < max {
            format!(" · starts at {min}")
        } else {
            String::new()
        };
        if self.has_teams {
            let side = max.div_ceil(2);
            let up_to = if min < max { "Up to " } else { "" };
            return format!("{up_to}{side} vs {side}{starts}");
        }
        let up_to = if min < max { "up to " } else { "" };
        format!("Free for all · {up_to}{max} players{starts}")
    }

    /// `enoughToStart`: the server's minimum, and in a team mode someone on
    /// each side, since the server will not launch a match with an empty side.
    #[must_use]
    pub fn enough_to_start(&self) -> bool {
        if self.players.len() < self.min_players {
            return false;
        }
        if !self.has_teams {
            return true;
        }
        let on = |t: &str| self.players.iter().any(|p| p.team == t);
        on("alpha") && on("bravo")
    }

    fn select_map_vote(&mut self, name: &str, client: &LobbyClient) {
        if self.phase == Phase::Starting {
            return;
        }
        // The server supports changing a vote, but not removing it.
        if self.maps.iter().any(|m| m.name == name && m.voted_by_local) {
            return;
        }
        for map in &mut self.maps {
            map.voted_by_local = map.name == name;
        }
        client.vote_map(&self.lobby_id, name);
    }

    fn set_ready(&mut self, ready: bool, client: &LobbyClient) {
        self.room_error = None;
        self.local_ready = ready;
        if let Some(p) = self.players.iter_mut().find(|p| p.is_local) {
            p.ready = ready;
        }
        client.set_ready(&self.lobby_id, ready);
    }

    /// `requestBot`: at `difficulty` (a preset), or without one at the
    /// server's choice for the inviter.
    fn request_bot(&mut self, client: &LobbyClient, difficulty: Option<&str>) {
        if self.phase == Phase::Starting || self.invite_bot_pending.is_some() || self.players.len() >= self.max_players
        {
            return;
        }
        self.invite_bot_pending = Some(Instant::now());
        client.invite_bot(&self.lobby_id, difficulty);
    }

    /// Quick play's next step: another bot while too few are seated. The
    /// player readies up themselves.
    fn continue_auto_fill(&mut self, client: &LobbyClient) {
        if self.auto_fill_bots == 0 {
            return;
        }
        if self.phase == Phase::Starting {
            self.auto_fill_bots = 0;
            return;
        }
        if self.bot_count() < self.auto_fill_bots && self.players.len() < self.max_players {
            self.request_bot(client, None);
            return;
        }
        self.auto_fill_bots = 0;
    }

    /// The status slab's phase: `searching`, `ready` or `starting`.
    fn status_phase(&self) -> StatusPhase {
        if self.phase == Phase::Starting {
            StatusPhase::Starting
        } else if self.enough_to_start() {
            StatusPhase::Ready
        } else {
            StatusPhase::Searching
        }
    }

    /// The slab's kicker, head and line under it (`refreshStatus`).
    #[must_use]
    pub fn status_text(&self) -> (&'static str, &'static str, String) {
        let full = self.players.len() >= self.max_players;
        let enough = self.enough_to_start();
        let starting = self.phase == Phase::Starting;
        let ready = self.players.iter().filter(|p| p.ready).count();
        let waiting = self.players.len() - ready;
        let open = self.max_players.saturating_sub(self.players.len());
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let kicker = if self.room_error.is_some() || self.match_in_progress || starting || !enough {
            "Match lobby"
        } else if full {
            "Lobby full"
        } else {
            "Enough to start"
        };
        let head = if self.room_error.is_some() {
            "Room action failed"
        } else if self.match_in_progress {
            "Match in progress"
        } else if starting {
            "Match starting"
        } else if !enough {
            if self.max_players == 2 {
                "Searching for an opponent"
            } else {
                "Waiting for players"
            }
        } else if self.local_ready {
            "You're ready"
        } else if full {
            "Ready up"
        } else {
            "Ready when you are"
        };
        let sub = if let Some(e) = &self.room_error {
            e.clone()
        } else if self.match_in_progress {
            "This room's match is still running. Rejoin your seat, or wait here for it to end.".into()
        } else if starting {
            "Everyone is ready. Connecting when the game server is available.".into()
        } else if !enough {
            if self.has_teams && self.players.len() >= self.min_players {
                if self.private_lobby {
                    "Each side needs a player. Switch sides, or add a bot.".into()
                } else {
                    "Each side needs a player. Switch sides, or wait for someone to join.".into()
                }
            } else if self.private_lobby {
                "Share the invite, or add a bot to play right now.".into()
            } else {
                "Share the room's link; it starts as soon as someone joins.".into()
            }
        } else if self.local_ready {
            format!("{waiting} player{} still to ready up.", plural(waiting))
        } else if full {
            format!("{ready} / {} ready · starts the moment everyone is", self.players.len())
        } else {
            format!(
                "{ready} / {} ready · {open} seat{} stay open until it starts",
                self.players.len(),
                plural(open)
            )
        };
        (kicker, head, sub)
    }

    /// A one-line summary, for logs and tests.
    #[must_use]
    pub fn status(&self) -> String {
        self.status_text().2
    }

    /// The Ready key's label and hint (`refreshReadyKey`).
    fn ready_text(&self) -> (&'static str, String) {
        let ready = self.players.iter().filter(|p| p.ready).count();
        let hint = if self.match_in_progress {
            "Match running".into()
        } else if self.phase == Phase::Starting {
            "Starting".into()
        } else if self.enough_to_start() {
            format!("{ready} / {} ready", self.players.len())
        } else if self.local_ready {
            "Waiting for the rest".into()
        } else {
            "Click when set".into()
        };
        (if self.local_ready { "Ready!" } else { "Ready" }, hint)
    }

    /// The Add bot key's hint (`updateInviteBotState`).
    fn bot_hint(&self) -> String {
        if let Some((note, _)) = &self.bot_note {
            return note.clone();
        }
        if self.invite_bot_pending.is_some() {
            return "Summoning…".into();
        }
        let open = self.max_players.saturating_sub(self.players.len());
        match open {
            0 => "Room is full".into(),
            1 => "1 seat open".into(),
            n => format!("{n} seats open"),
        }
    }

    /// `startClock`: seconds in the room, so the wait is visibly real.
    fn clock(&self) -> String {
        if self.match_in_progress {
            return "In match".into();
        }
        if self.phase == Phase::Starting {
            return "Starting".into();
        }
        let s = self.opened_at.elapsed().as_secs();
        format!("Waiting {}:{:02}", s / 60, s % 60)
    }
}

/// The slab's state (`data-phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusPhase {
    Searching,
    Ready,
    Starting,
}

/// How a seat sets its parts: one row, a grid of two rows, or the duel's card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeatKind {
    Row,
    Grid,
    Vs,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Value {
        serde_json::json!({
            "id": "L1", "gametype": "ffa", "visibility": "private",
            "config": {"maxPlayers": 6, "minPlayers": 2, "hasTeams": false},
            "maps": [{"name": "washoi_dm2", "displayName": "Dojo", "votes": 0}],
            "players": [{"id": "me", "name": "Washiki", "team": "ffa", "ready": false}]
        })
    }

    fn joined(id: &str) -> LobbyIn {
        LobbyIn::Other {
            kind: "lobby:player_joined".into(),
            message: serde_json::json!({"player": {"id": id, "name": id, "team": "ffa", "ready": true, "botLevel": "normal"}}),
        }
    }

    #[test]
    fn quick_play_adds_its_bots_one_at_a_time_and_leaves_ready_to_the_player() {
        // Nothing listens there: the client queues and drops what it sends.
        let client = LobbyClient::connect("ws://127.0.0.1:9");
        let mut view = MatchLobbyView::new(&room(), Some("me"), 2, 0, &client);
        assert!(view.maps[0].voted_by_local);
        assert!(view.invite_bot_pending.is_some());
        assert!(!view.local_ready);
        view.handle(&joined("b1"), &client);
        assert!(view.invite_bot_pending.is_some());
        assert!(!view.local_ready);
        view.handle(&joined("b2"), &client);
        assert_eq!(view.bot_count(), 2);
        assert_eq!(view.auto_fill_bots, 0);
        assert!(!view.local_ready);
    }

    #[test]
    fn a_finished_match_returns_the_room_to_ready_up() {
        let client = LobbyClient::connect("ws://127.0.0.1:9");
        let mut view = MatchLobbyView::new(&room(), Some("me"), 0, 0, &client);
        let other = |kind: &str| LobbyIn::Other {
            kind: kind.into(),
            message: Value::Null,
        };
        view.handle(&other("lobby:all_ready"), &client);
        assert_eq!(view.phase, Phase::Starting);
        view.handle(&other("match:ended"), &client);
        assert_eq!(view.phase, Phase::Waiting);
        assert!(view.players.iter().all(|p| !p.ready));
    }

    #[test]
    fn the_room_says_what_it_waits_on() {
        let client = LobbyClient::connect("ws://127.0.0.1:9");
        let mut view = MatchLobbyView::new(&room(), Some("me"), 0, 0, &client);
        assert_eq!(view.shape(), "Free for all · up to 6 players · starts at 2");
        let (kicker, head, sub) = view.status_text();
        assert_eq!((kicker, head), ("Match lobby", "Waiting for players"));
        assert_eq!(sub, "Share the invite, or add a bot to play right now.");
        view.handle(&joined("b1"), &client);
        let (kicker, head, sub) = view.status_text();
        assert_eq!((kicker, head), ("Enough to start", "Ready when you are"));
        assert_eq!(sub, "1 / 2 ready · 4 seats stay open until it starts");
    }

    #[test]
    fn an_invite_opens_on_the_game_s_site() {
        assert_eq!(
            invite_href("https://eu.washoi.net/", "/?lobby=L1&invite=x", "L1"),
            "https://eu.washoi.net/?lobby=L1&invite=x"
        );
        assert_eq!(invite_href("http://h", "", "L2"), "http://h/?lobby=L2");
        assert_eq!(invite_href("http://h", "https://o/?lobby=1", "1"), "https://o/?lobby=1");
    }
}
