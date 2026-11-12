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
    /// Quick play: bots still to add before this player readies up; 0 when off.
    auto_fill_bots: usize,
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

    /// `requestBot`.
    fn request_bot(&mut self, client: &LobbyClient) {
        if self.phase == Phase::Starting || self.invite_bot_pending.is_some() || self.players.len() >= self.max_players
        {
            return;
        }
        self.invite_bot_pending = Some(Instant::now());
        client.invite_bot(&self.lobby_id, None);
    }

    /// Quick play's next step: another bot while too few are seated, then ready up.
    fn continue_auto_fill(&mut self, client: &LobbyClient) {
        if self.auto_fill_bots == 0 {
            return;
        }
        if self.phase == Phase::Starting {
            self.auto_fill_bots = 0;
            return;
        }
        if self.bot_count() < self.auto_fill_bots && self.players.len() < self.max_players {
            self.request_bot(client);
            return;
        }
        // Bots ready up on their own, so this player readies last: earlier,
        // the room could start as soon as it reached its minimum, short of bots.
        self.auto_fill_bots = 0;
        if !self.local_ready {
            self.set_ready(true, client);
        }
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

    /// Draws the room in `rect`; what the panel must do, when the player asks.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
    ) -> Option<RoomAction> {
        self.tick();
        let painter = ui.painter_at(rect.expand2(vec2(40.0, 24.0)));
        let time = ui.input(|i| i.time);
        let s = Sizes::new(style);
        let mut action = None;

        let top_h = self.top_bar(ui, &painter, rect, style);
        let status_top = rect.top() + top_h + s.gap;
        let status_h = self.status_slab(&painter, rect, status_top, style, time);
        let actions_h = s.ready_h + 10.2 + 4.0;
        let actions_top = rect.bottom() - actions_h;
        let body = Rect::from_min_max(
            pos2(rect.left(), status_top + status_h + s.gap),
            pos2(rect.right(), actions_top - s.gap),
        );
        self.body(ui, body, client, style, &s, time);
        if let Some(a) = self.actions(ui, &painter, rect, client, style, &s, time) {
            action = Some(a);
        }
        // Keep the radar turning.
        ui.ctx().request_repaint();
        action
    }

    /// `match-lobby__top`: the mode chip, the shape and id, and the invite.
    fn top_bar(&mut self, ui: &mut egui::Ui, painter: &Painter, rect: Rect, style: &RoomStyle<'_>) -> f32 {
        let a = style.accent;
        let chip_size = clamp_css(18.0, style.window_width * 0.016, 26.0);
        let label = text(
            painter,
            &self.gametype_label,
            chip_size,
            style.display.clone(),
            ink(1.0),
            chip_size * 0.06,
        );
        let chip_w = label.size().x + 48.0;
        let chip_h = label.size().y.max(chip_size) + 20.0;
        let h = chip_h.max(46.0);
        let chip = Rect::from_min_size(pos2(rect.left(), rect.top() + (h - chip_h) / 2.0), vec2(chip_w, chip_h));
        let slant = chip_w * 0.06;
        gradient_polygon(
            painter,
            &[
                chip.left_top() + vec2(slant, 0.0),
                chip.right_top(),
                chip.right_bottom() - vec2(slant, 0.0),
                chip.left_bottom(),
            ],
            &[(0.0, a.hi), (0.6, a.face), (1.0, a.lo)],
        );
        painter.galley(
            pos2(chip.left() + 22.0, chip.center().y - label.size().y / 2.0),
            label,
            ink(1.0),
        );
        // `match-lobby__invite`, on the right: label, the link, Copy.
        let href = invite_href(style.site, &self.share_url, &self.lobby_id);
        let shown = href
            .split_once("://")
            .map_or(href.as_str(), |(_, rest)| rest)
            .to_owned();
        let copy_label = if self.copied.is_some() { "Copied" } else { "Copy" };
        let copy_text = text(
            painter,
            &copy_label.to_uppercase(),
            15.0,
            style.display.clone(),
            ink(1.0),
            1.0,
        );
        let copy_w = copy_text.size().x + 40.0;
        let copy_h = 41.0;
        let copy = Rect::from_min_size(
            pos2(rect.right() - copy_w - 4.0, rect.top() + (h - copy_h) / 2.0 - 2.0),
            vec2(copy_w, copy_h),
        );
        let face = RaisedKey::new(("room-copy", &self.lobby_id), copy, KeyLook::cream().sm()).show(ui);
        KeyText::new(copy_label, "", style.display.clone(), style.body.clone())
            .size(15.0, 12.0)
            .spacing(0.07)
            .pad(20.0, 20.0)
            .layout(KeyTextLayout::Centered)
            .paint(painter, &face);
        if face.clicked {
            ui.ctx().copy_text(href.clone());
            self.copied = Some(Instant::now());
        }
        let meta_right = chip.right() + 18.0 + 200.0;
        let mut invite_left = copy.left();
        let url_max = (style.window_width * 0.32).clamp(80.0, 46.0 * 15.0 * 0.62);
        let url_room = (copy.left() - 12.0 - meta_right - 80.0).clamp(0.0, url_max);
        if url_room > 60.0 {
            let elided = elide(painter, &shown, 15.0, style.display.clone(), url_room - 36.0);
            let url = text(painter, &elided, 15.0, style.display.clone(), cream(), 15.0 * 0.04);
            let url_w = url.size().x + 36.0;
            let field = Rect::from_min_size(
                pos2(copy.left() - 12.0 - url_w, rect.top() + (h - copy_h) / 2.0),
                vec2(url_w, copy_h),
            );
            let slab = [
                field.left_top(),
                field.right_top(),
                field.right_bottom() - vec2(10.0, 0.0),
                field.left_bottom(),
            ];
            painter.add(Shape::convex_polygon(
                slab.to_vec(),
                hsla(250.0, 44.0, 5.0, 0.8),
                Stroke::NONE,
            ));
            painter.add(Shape::closed_line(
                slab.to_vec(),
                Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.3)),
            ));
            painter.galley(
                pos2(field.left() + 18.0, field.center().y - url.size().y / 2.0),
                url,
                cream(),
            );
            let label = text(
                painter,
                "INVITE",
                12.0,
                style.body.clone(),
                hsla(45.0, 100.0, 95.0, 0.5),
                12.0 * 0.26,
            );
            let lx = field.left() - 12.0 - label.size().x;
            invite_left = field.left();
            if lx > meta_right {
                invite_left = lx;
                painter.galley(pos2(lx, field.center().y - label.size().y / 2.0), label, cream());
            }
        }
        // `match-lobby__meta`: the shape over the room's id, wrapped in what
        // the invite leaves.
        let faint = hsla(45.0, 100.0, 95.0, 0.6);
        let meta_x = chip.right() + 18.0;
        let meta_w = (invite_left - 24.0 - meta_x).max(120.0);
        let line_h = 14.0 * 1.3;
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &self.shape().to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::new(14.0, style.body.clone()),
                color: faint,
                extra_letter_spacing: 14.0 * 0.16,
                line_height: Some(line_h),
                ..Default::default()
            },
        );
        job.wrap.max_width = meta_w;
        let shape = painter.layout_job(job);
        let lines = shape.rows.len().max(1) as f32 + 1.0;
        let meta_top = rect.top() + h / 2.0 - lines * line_h / 2.0;
        let sh = shape.size().y;
        painter.galley(pos2(meta_x, meta_top), shape, faint);
        let lobby = text(painter, "LOBBY ", 14.0, style.body.clone(), faint, 14.0 * 0.16);
        let lw = lobby.size().x;
        painter.galley(pos2(meta_x, meta_top + sh), lobby, faint);
        let id = text(
            painter,
            &self.lobby_id.to_uppercase(),
            14.0,
            style.display.clone(),
            cream(),
            14.0 * 0.08,
        );
        painter.galley(pos2(meta_x + lw, meta_top + sh + 1.0), id, cream());
        h
    }

    /// `match-lobby__status`: the radar, what the room waits on, the count and the fill.
    fn status_slab(&self, painter: &Painter, rect: Rect, top: f32, style: &RoomStyle<'_>, time: f64) -> f32 {
        let a = style.accent;
        let phase = self.status_phase();
        let (kicker, head, sub) = self.status_text();
        let error = self.room_error.is_some();
        let (pad_t, pad_b, pad_l, pad_r) = if style.short {
            (12.0, 18.0, 20.0, 30.0)
        } else {
            (20.0, 26.0, 24.0, 34.0)
        };
        let radar = if style.short { 78.0 } else { 104.0 };
        let gap = if style.short { 20.0 } else { 28.0 };
        let head_size = if style.short {
            clamp_css(22.0, style.window_width * 0.021, 38.0)
        } else {
            clamp_css(26.0, style.window_width * 0.026, 46.0)
        };
        let sub_size = if style.short { 17.0 } else { 19.0 };
        let count_size = if style.short { 48.0 } else { 60.0 };
        let of_size = if style.short { 24.0 } else { 28.0 };

        // The tally on the right decides how wide the words may run.
        let count = text(
            painter,
            &self.players.len().to_string(),
            count_size,
            style.display.clone(),
            cream(),
            0.0,
        );
        let of = text(
            painter,
            &format!("/{}", self.max_players),
            of_size,
            style.display.clone(),
            hsla(45.0, 100.0, 95.0, 0.42),
            0.0,
        );
        let clock = text(
            painter,
            &self.clock().to_uppercase(),
            13.0,
            style.body.clone(),
            hsla(45.0, 100.0, 95.0, 0.5),
            13.0 * 0.2,
        );
        let tally_w = (count.size().x + of.size().x).max(clock.size().x);
        let words_left = rect.left() + 10.0 + pad_l + radar + gap;
        let words_w = (rect.right() - pad_r - tally_w - gap - words_left).max(80.0);
        let head_color = if error {
            hsl(340.0, 100.0, 72.0)
        } else if phase == StatusPhase::Ready {
            a.hi
        } else {
            cream()
        };
        let sub_color = if error {
            hsl(340.0, 100.0, 72.0)
        } else {
            hsla(45.0, 100.0, 95.0, 0.78)
        };
        let kicker = text(
            painter,
            &kicker.to_uppercase(),
            12.0,
            style.body.clone(),
            a.face,
            12.0 * 0.32,
        );
        let head_shown = elide(painter, &head.to_uppercase(), head_size, style.display.clone(), words_w);
        let head = text(
            painter,
            &head_shown,
            head_size,
            style.display.clone(),
            head_color,
            head_size * 0.02,
        );
        let mut job =
            egui::text::LayoutJob::simple(sub, egui::FontId::new(sub_size, style.body.clone()), sub_color, words_w);
        job.wrap.max_rows = 2;
        let sub = painter.layout_job(job);
        let words_h = kicker.size().y + 8.0 + head.size().y + 10.0 + sub.size().y;
        let h = pad_t + words_h.max(radar) + pad_b;
        let slab = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), h));

        // The slab, cut off its top right, on its accent edge.
        let cut = 26.0;
        let shape = [
            slab.left_top(),
            pos2(slab.right() - cut, slab.top()),
            pos2(slab.right(), slab.top() + cut),
            slab.right_bottom(),
            slab.left_bottom(),
        ];
        let tinted = hsl(250.0, 40.0, 10.0).lerp_to_gamma(a.face, 0.16);
        gradient_polygon_x(painter, &shape, &[(0.0, tinted), (0.55, hsla(250.0, 44.0, 7.0, 0.92))]);
        painter.rect_filled(Rect::from_min_size(slab.min, vec2(10.0, h)), 0.0, a.face);
        // A light band sweeping across while the room waits.
        let period = match phase {
            StatusPhase::Searching => 3.2,
            StatusPhase::Ready => 1.6,
            StatusPhase::Starting => 0.9,
        };
        let t = ease_in_out(((time / period).fract()) as f32);
        let band_x = slab.left() + slab.width() * (t * 2.0 - 1.0);
        let band = Rect::from_min_size(pos2(band_x, slab.top()), slab.size());
        let clipped = painter.with_clip_rect(slab.intersect(painter.clip_rect()));
        horizontal_band(
            &clipped,
            band,
            &[
                (0.3, Color32::TRANSPARENT),
                (0.5, a.face.gamma_multiply(0.14)),
                (0.7, Color32::TRANSPARENT),
            ],
        );

        // The radar.
        let centre = pos2(slab.left() + 10.0 + pad_l + radar / 2.0, slab.top() + h / 2.0 - 3.0);
        self.paint_radar(painter, centre, radar, phase, &a, style, time);

        // The words.
        let wy = slab.top() + (h - pad_b + pad_t) / 2.0 - words_h / 2.0;
        let kh = kicker.size().y;
        painter.galley(pos2(words_left, wy), kicker, a.face);
        let hy = wy + kh + 8.0;
        let hh = head.size().y;
        painter.galley(pos2(words_left + 4.0, hy + 5.0), head.clone(), hsl(250.0, 50.0, 3.0));
        painter.galley(pos2(words_left, hy), head, head_color);
        painter.galley(pos2(words_left, hy + hh + 10.0), sub, sub_color);

        // The tally.
        let right = slab.right() - pad_r;
        let total_h = count.size().y * 0.9 + 8.0 + clock.size().y;
        let ty = slab.top() + (h - pad_b + pad_t) / 2.0 - total_h / 2.0;
        let ow = of.size().x;
        let cw = count.size().x;
        let ch = count.size().y;
        painter.galley(pos2(right - ow, ty + ch - of.size().y - ch * 0.08), of, cream());
        painter.galley(pos2(right - ow - cw, ty), count, cream());
        painter.galley(pos2(right - clock.size().x, ty + ch * 0.9 + 8.0), clock, cream());

        // `match-lobby__status-bar`: how full the room is.
        let bar = Rect::from_min_max(pos2(slab.left(), slab.bottom() - 8.0), slab.right_bottom());
        painter.rect_filled(bar, 0.0, hsla(0.0, 0.0, 100.0, 0.06));
        let share = (self.players.len() as f32 / self.max_players.max(1) as f32).min(1.0);
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * share, 8.0));
        if fill.width() > 0.5 {
            painter.add(gradient_rect_x(fill, &[(0.0, a.lo), (1.0, a.hi)]));
            let waiting = !error
                && !self.match_in_progress
                && (self.phase == Phase::Starting || !self.enough_to_start() || self.local_ready);
            let crawl = if !waiting {
                0.0
            } else if phase == StatusPhase::Starting {
                (time / 0.4).fract() as f32
            } else {
                (time / 0.8).fract() as f32
            };
            let stripes = painter.with_clip_rect(fill.intersect(painter.clip_rect()));
            let dark = hsla(250.0, 60.0, 5.0, 0.28);
            let mut x = fill.left() - 28.0 + crawl * 28.0;
            while x < fill.right() + 8.0 {
                stripes.add(Shape::convex_polygon(
                    vec![
                        pos2(x, fill.bottom()),
                        pos2(x + 8.0, fill.top()),
                        pos2(x + 15.0, fill.top()),
                        pos2(x + 7.0, fill.bottom()),
                    ],
                    dark,
                    Stroke::NONE,
                ));
                x += 14.0;
            }
        }
        h
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_radar(
        &self,
        painter: &Painter,
        c: Pos2,
        size: f32,
        phase: StatusPhase,
        a: &Accent,
        style: &RoomStyle<'_>,
        time: f64,
    ) {
        let r = size / 2.0;
        let k = size / 104.0;
        painter.circle_filled(c, r, hsla(250.0, 44.0, 5.0, 0.4));
        painter.circle_filled(c, r * 0.6 / 0.62, hsla(250.0, 44.0, 5.0, 0.9));
        painter.circle_stroke(c, r - 1.0, Stroke::new(2.0, a.face.gamma_multiply(0.55)));
        let ring = Stroke::new(1.5, a.face.gamma_multiply(0.35));
        let core_inset = match phase {
            StatusPhase::Searching => 45.0,
            StatusPhase::Ready => 16.0,
            StatusPhase::Starting => 30.0,
        } * k;
        if phase == StatusPhase::Ready {
            // The core swells into a signal that pulses outward.
            for delay in [0.0, 0.7] {
                let t = (((time + delay) / 1.4).fract()) as f32;
                let radius = (r - 16.0 * k) * (1.0 + 0.55 * t);
                painter.circle_stroke(c, radius, Stroke::new(3.0, a.face.gamma_multiply(0.9 * (1.0 - t))));
            }
        } else {
            painter.circle_stroke(c, r - 18.0 * k, ring);
            painter.circle_stroke(c, r - 36.0 * k, ring);
            // The sweep: a conic tail behind a turning edge.
            let period = if phase == StatusPhase::Starting { 0.7 } else { 2.2 };
            let turn = (time / period).fract() as f32 * std::f32::consts::TAU;
            let mut mesh = Mesh::default();
            let steps = 28;
            for i in 0..steps {
                let t0 = i as f32 / steps as f32;
                let t1 = (i + 1) as f32 / steps as f32;
                // `conic-gradient(transparent 0 70%, accent 100%)`
                let at = |t: f32| turn - std::f32::consts::TAU * 0.3 * (1.0 - t);
                let color = |t: f32| a.face.gamma_multiply(0.85 * t);
                let base = mesh.vertices.len() as u32;
                // CSS turns clockwise from the top.
                let point = |ang: f32| c + vec2(ang.sin(), -ang.cos()) * (r - 1.0);
                mesh.colored_vertex(c, color(t0));
                mesh.colored_vertex(point(at(t0)), color(t0));
                mesh.colored_vertex(point(at(t1)), color(t1));
                mesh.add_triangle(base, base + 1, base + 2);
            }
            painter.add(mesh);
            if phase == StatusPhase::Searching {
                for (dx, dy, delay) in [(66.0, 24.0, 0.0), (22.0, 62.0, 1.3)] {
                    let t = ((time + delay) / 2.2).fract();
                    let alpha = if (0.30..0.34).contains(&t) {
                        ((t - 0.30) / 0.04) as f32
                    } else if (0.34..0.8).contains(&t) {
                        1.0 - ((t - 0.34) / 0.46) as f32
                    } else {
                        0.0
                    };
                    if alpha > 0.0 {
                        let p = c - vec2(r, r) + vec2(dx + 4.5, dy + 4.5) * k;
                        painter.circle_filled(p, 9.0 * k, a.face.gamma_multiply(0.3 * alpha));
                        painter.circle_filled(p, 4.5 * k, a.hi.gamma_multiply(alpha));
                    }
                }
            }
        }
        let core_r = r - core_inset;
        painter.circle_filled(c, core_r, a.face);
        let mark = if self.enough_to_start() {
            "!".to_owned()
        } else {
            self.min_players.saturating_sub(self.players.len()).max(1).to_string()
        };
        let g = text(painter, &mark, 34.0 * k, style.display.clone(), ink(1.0), 0.0);
        painter.galley(c - g.size() / 2.0, g, ink(1.0));
    }

    /// The roster and the map vote, which scroll between them when short of room.
    fn body(
        &mut self,
        ui: &mut egui::Ui,
        body: Rect,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
        s: &Sizes,
        time: f64,
    ) {
        if body.height() < 20.0 {
            return;
        }
        let roster_h = self.roster_height(body.width(), style, s);
        let vote_head = 20.0 + 12.0;
        let map_rows = if style.narrower { 2.0 } else { 1.0 };
        let maps_min = if style.narrower {
            260.0
        } else if style.short {
            124.0
        } else {
            150.0
        };
        let fit = body.height() - roster_h - s.gap - vote_head;
        let maps_h = fit.max(maps_min) * if style.narrower { 1.0 } else { map_rows };
        let content_h = roster_h + s.gap + vote_head + maps_h;
        // The keys lift up and left and cast their side down and right, so the
        // scroll box reaches past the room on those edges.
        let outer = Rect::from_min_max(body.min - vec2(8.0, 8.0), body.max + vec2(24.0, 14.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(outer), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("room-body", &self.lobby_id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let (area, _) =
                        ui.allocate_exact_size(vec2(outer.width() - 16.0, content_h + 22.0), egui::Sense::hover());
                    let inner = Rect::from_min_size(area.min + vec2(8.0, 8.0), vec2(body.width(), content_h));
                    let painter = ui.painter().clone();
                    self.roster(&painter, inner, style, s, time);
                    let vote_top = inner.top() + roster_h + s.gap;
                    self.vote(ui, &painter, inner, vote_top, maps_h, client, style);
                });
        });
    }

    fn roster_height(&self, width: f32, style: &RoomStyle<'_>, s: &Sizes) -> f32 {
        if self.max_players == 2 {
            return s.vs_seat_h;
        }
        if self.has_teams {
            let per_team = self.max_players.div_ceil(2) as f32;
            let head = if style.short { 17.0 } else { 22.0 };
            let gap = if style.short { 6.0 } else { 8.0 };
            let seat = self.team_seat_h(width, style);
            return head + gap + per_team * seat + (per_team - 1.0) * gap;
        }
        let cols = if style.narrower { 2 } else { 4 };
        let rows = self.max_players.div_ceil(cols) as f32;
        rows * s.ffa_seat_h + (rows - 1.0) * 14.0
    }

    fn team_seat_h(&self, width: f32, style: &RoomStyle<'_>) -> f32 {
        let column = (width - 22.0 * 2.0 - 40.0) / 2.0;
        match (style.short, column < 380.0) {
            (true, _) => 36.0,
            (false, true) => 52.0,
            (false, false) => 46.0,
        }
    }

    fn roster(&self, painter: &Painter, r: Rect, style: &RoomStyle<'_>, s: &Sizes, time: f64) {
        let (you, them) = (hsl(322.0, 95.0, 62.0), hsl(182.0, 95.0, 52.0));
        if self.max_players == 2 {
            // Two seats face each other across the VS.
            let vs = vs_galley(painter, clamp_css(40.0, style.window_width * 0.036, 64.0), style);
            let vs_w = vs.size().x + 12.0;
            let seat_w = (r.width() - vs_w - 44.0) / 2.0;
            let mine = self.players.iter().find(|p| p.is_local).or(self.players.first());
            let other = self.players.iter().find(|p| Some(*p) != mine);
            let left = Rect::from_min_size(r.min, vec2(seat_w, s.vs_seat_h));
            let right = Rect::from_min_size(pos2(r.right() - seat_w, r.top()), vec2(seat_w, s.vs_seat_h));
            self.seat(painter, left, mine, you, SeatKind::Vs, 0, style, time);
            paint_vs(painter, pos2(r.center().x, left.center().y), vs);
            self.seat(painter, right, other, them, SeatKind::Vs, 1, style, time);
            return;
        }
        if self.has_teams {
            let vs = vs_galley(painter, 34.0, style);
            let vs_w = vs.size().x + 12.0;
            let col_w = (r.width() - vs_w - 44.0) / 2.0;
            let local = self.local_team();
            let per_team = self.max_players.div_ceil(2);
            let seat_h = self.team_seat_h(r.width(), style);
            let mut seats_top = r.top();
            for (i, team) in ["alpha", "bravo"].into_iter().enumerate() {
                let x = if i == 0 { r.left() } else { r.right() - col_w };
                let mine = local == Some(team);
                let colour = if mine { you } else { them };
                seats_top = self.team_column(
                    painter,
                    Rect::from_min_size(pos2(x, r.top()), vec2(col_w, r.height())),
                    team,
                    mine,
                    colour,
                    per_team,
                    seat_h,
                    style,
                    time,
                );
            }
            let seats_bottom = r.top() + self.roster_height(r.width(), style, &Sizes::new(style));
            paint_vs(painter, pos2(r.center().x, (seats_top + seats_bottom) / 2.0), vs);
            return;
        }
        let cols = if style.narrower { 2 } else { 4 };
        let w = (r.width() - 22.0 * (cols as f32 - 1.0)) / cols as f32;
        for i in 0..self.max_players {
            let (row, col) = (i / cols, i % cols);
            let rect = Rect::from_min_size(
                pos2(
                    r.left() + col as f32 * (w + 22.0),
                    r.top() + row as f32 * (s.ffa_seat_h + 14.0),
                ),
                vec2(w, s.ffa_seat_h),
            );
            let player = self.players.get(i);
            let colour = if player.is_some_and(|p| p.is_local) { you } else { them };
            self.seat(painter, rect, player, colour, SeatKind::Grid, i, style, time);
        }
    }

    /// `buildTeam`; returns where its seats start.
    #[allow(clippy::too_many_arguments)]
    fn team_column(
        &self,
        painter: &Painter,
        r: Rect,
        team: &str,
        mine: bool,
        colour: Color32,
        seats: usize,
        seat_h: f32,
        style: &RoomStyle<'_>,
        time: f64,
    ) -> f32 {
        let head_size = if style.short { 15.0 } else { 18.0 };
        let gap = if style.short { 6.0 } else { 8.0 };
        let name = text(
            painter,
            &self.team_name(team).to_uppercase(),
            head_size,
            style.display.clone(),
            colour,
            head_size * 0.1,
        );
        let base = r.top() + name.size().y;
        let mut x = r.left() + 4.0;
        let nw = name.size().x;
        painter.galley(pos2(x, r.top()), name, colour);
        x += nw + 12.0;
        if mine {
            let tag = text(
                painter,
                "YOUR SIDE",
                11.0,
                style.body.clone(),
                hsla(45.0, 100.0, 95.0, 0.55),
                11.0 * 0.22,
            );
            painter.galley(pos2(x, base - tag.size().y), tag, cream());
        }
        let members: Vec<&MatchLobbyPlayer> = self.players.iter().filter(|p| p.team == team).collect();
        let tally = text(
            painter,
            &format!("{}/{seats}", members.len()),
            15.0,
            style.display.clone(),
            hsla(45.0, 100.0, 95.0, 0.7),
            0.0,
        );
        painter.galley(
            pos2(r.right() - 4.0 - tally.size().x, base - tally.size().y),
            tally,
            cream(),
        );
        let top = r.top() + head_size + 4.0 + gap;
        let kind = if r.width() < 380.0 {
            SeatKind::Grid
        } else {
            SeatKind::Row
        };
        for i in 0..seats {
            let rect = Rect::from_min_size(pos2(r.left(), top + i as f32 * (seat_h + gap)), vec2(r.width(), seat_h));
            self.seat(painter, rect, members.get(i).copied(), colour, kind, i, style, time);
        }
        top
    }

    /// `buildSeat`.
    #[allow(clippy::too_many_arguments)]
    fn seat(
        &self,
        painter: &Painter,
        r: Rect,
        player: Option<&MatchLobbyPlayer>,
        colour: Color32,
        kind: SeatKind,
        index: usize,
        style: &RoomStyle<'_>,
        time: f64,
    ) {
        let vs = kind == SeatKind::Vs;
        let slant = if vs { 22.0 } else { 12.0 };
        let border = if vs { 8.0 } else { 5.0 };
        let shape = [
            r.left_top(),
            r.right_top(),
            pos2(r.right() - slant, r.bottom()),
            r.left_bottom(),
        ];
        let open = player.is_none();
        let colour = if open { hsla(45.0, 100.0, 95.0, 0.22) } else { colour };
        if open {
            painter.add(Shape::convex_polygon(
                shape.to_vec(),
                hsla(250.0, 44.0, 6.0, 0.55),
                Stroke::NONE,
            ));
            // Hatched.
            let hatch = painter.with_clip_rect(r.intersect(painter.clip_rect()));
            let mut x = r.left() - r.height();
            while x < r.right() {
                hatch.add(Shape::convex_polygon(
                    vec![
                        pos2(x, r.bottom()),
                        pos2(x + r.height(), r.top()),
                        pos2(x + r.height() + 7.0, r.top()),
                        pos2(x + 7.0, r.bottom()),
                    ],
                    hsla(0.0, 0.0, 100.0, 0.035),
                    Stroke::NONE,
                ));
                x += 14.0;
            }
            // Dashed edge.
            let mut y = r.top();
            while y < r.bottom() {
                let to = (y + 6.0).min(r.bottom());
                painter.rect_filled(
                    Rect::from_min_max(pos2(r.left(), y), pos2(r.left() + border, to)),
                    0.0,
                    colour,
                );
                y += 10.0;
            }
            // A shimmer running through it while the room waits.
            let t = ease_in_out((((time - index as f64 * 0.18) / 2.4).rem_euclid(1.0)) as f32);
            let band = Rect::from_min_size(pos2(r.left() + r.width() * (t * 2.0 - 1.0), r.top()), r.size());
            horizontal_band(
                &hatch,
                band,
                &[
                    (0.35, Color32::TRANSPARENT),
                    (0.5, style.accent.face.gamma_multiply(0.18)),
                    (0.65, Color32::TRANSPARENT),
                ],
            );
        } else {
            let tinted = hsl(250.0, 40.0, 9.0).lerp_to_gamma(colour, 0.24);
            gradient_polygon_x(painter, &shape, &[(0.0, tinted), (0.7, hsla(250.0, 44.0, 7.0, 0.92))]);
            painter.rect_filled(
                Rect::from_min_max(r.min, pos2(r.left() + border, r.bottom())),
                0.0,
                colour,
            );
        }

        let (avatar, avatar_font, name_size) = match kind {
            SeatKind::Vs if style.narrow => (54.0, 24.0, clamp_css(16.0, style.window_width * 0.015, 26.0)),
            SeatKind::Vs => (76.0, 34.0, clamp_css(20.0, style.window_width * 0.017, 30.0)),
            SeatKind::Grid if style.short => (30.0, 14.0, 16.0),
            SeatKind::Grid => (36.0, 14.0, 16.0),
            SeatKind::Row if style.short => (24.0, 12.0, 16.0),
            SeatKind::Row => (30.0, 14.0, 16.0),
        };
        let pad_l = border + if vs { 16.0 } else { 12.0 };
        let pad_r = slant + if vs { 10.0 } else { 8.0 };
        let gap = if vs { 20.0 } else { 12.0 };
        let av = Rect::from_center_size(
            pos2(r.left() + pad_l + avatar / 2.0, r.center().y),
            vec2(avatar, avatar),
        );
        let av_shape = vec![
            av.left_top() + vec2(avatar * 0.14, 0.0),
            av.right_top() + vec2(0.0, avatar * 0.08),
            av.right_bottom() - vec2(avatar * 0.12, 0.0),
            av.left_bottom() - vec2(0.0, avatar * 0.1),
        ];
        let x0 = av.right() + gap;
        let right = r.right() - pad_r;
        let Some(p) = player else {
            let pulse = 0.5 + 0.5 * (time * std::f64::consts::TAU / 1.6).cos() as f32;
            let scale = 0.84 + 0.16 * pulse;
            let small = Rect::from_center_size(av.center(), av.size() * scale);
            painter.rect_filled(small, 0.0, hsla(0.0, 0.0, 100.0, 0.07));
            let q = text(
                painter,
                "?",
                avatar_font * scale,
                style.display.clone(),
                hsla(45.0, 100.0, 95.0, 0.45 * (0.55 + 0.45 * pulse)),
                0.0,
            );
            painter.galley(av.center() - q.size() / 2.0, q, cream());
            let label = if self.phase == Phase::Starting {
                "EMPTY SEAT"
            } else if self.max_players == 2 {
                "WAITING FOR A PLAYER"
            } else {
                "OPEN"
            };
            let size = if vs { name_size * 0.82 } else { 16.0 };
            let mut job = egui::text::LayoutJob::default();
            job.append(
                label,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(size, style.body.clone()),
                    color: hsla(45.0, 100.0, 95.0, 0.4),
                    italics: true,
                    extra_letter_spacing: size * 0.1,
                    ..Default::default()
                },
            );
            let g = painter.layout_job(job);
            painter.galley(pos2(x0, r.center().y - g.size().y / 2.0), g, cream());
            return;
        };
        gradient_polygon(
            painter,
            &av_shape,
            &[(0.0, colour), (1.0, colour.lerp_to_gamma(hsl(276.0, 90.0, 64.0), 0.6))],
        );
        let initial: String = p
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let g = text(painter, &initial, avatar_font, style.display.clone(), ink(1.0), 0.0);
        painter.galley(av.center() - g.size() / 2.0, g, ink(1.0));

        // Tags: "You", a bot's level.
        let mut tags = Vec::new();
        if p.is_local {
            tags.push(("YOU".to_owned(), hsl(46.0, 100.0, 60.0)));
        }
        if let Some(level) = &p.bot_level {
            tags.push((
                format!("BOT · {}", capitalised(level).to_uppercase()),
                hsl(186.0, 90.0, 46.0),
            ));
        }
        let tag_galleys: Vec<_> = tags
            .iter()
            .map(|(t, c)| (text(painter, t, 10.0, style.display.clone(), ink(1.0), 1.2), *c))
            .collect();
        let tags_w: f32 = tag_galleys.iter().map(|(g, _)| g.size().x + 16.0 + 6.0).sum();

        let in_match = self.match_in_progress;
        let ready = p.ready && !in_match;
        let state = if in_match {
            "IN MATCH"
        } else if p.ready {
            "READY"
        } else {
            "NOT READY"
        };
        let go = hsl(96.0, 85.0, 55.0);
        let two_rows = kind != SeatKind::Row;
        let (state_size, state_chip) = match kind {
            SeatKind::Vs if style.narrow => (11.0, true),
            SeatKind::Vs => (13.0, true),
            SeatKind::Grid => (10.0, false),
            SeatKind::Row => (11.0, true),
        };
        let state_color = if ready && state_chip {
            ink(1.0)
        } else if ready {
            go
        } else {
            hsla(45.0, 100.0, 95.0, 0.42)
        };
        let state_g = text(
            painter,
            state,
            state_size,
            style.display.clone(),
            state_color,
            state_size * 0.1,
        );
        let check = if ready { state_size * 0.9 + 4.0 } else { 0.0 };
        let (chip_px, chip_py) = match kind {
            SeatKind::Vs if !style.narrow => (14.0, 8.0),
            _ => (11.0, 5.0),
        };
        let state_w = state_g.size().x + check + if state_chip { chip_px * 2.0 } else { 0.0 };
        let state_h = state_g.size().y + if state_chip { chip_py * 2.0 } else { 0.0 };

        let name_room = if two_rows {
            right - x0 - tags_w
        } else {
            right - x0 - tags_w - state_w - 12.0
        };
        let weight_size = name_size;
        let shown = elide(
            painter,
            &p.name,
            weight_size,
            style.display.clone(),
            name_room.max(20.0),
        );
        let name = text(
            painter,
            &shown,
            weight_size,
            style.display.clone(),
            cream(),
            weight_size * 0.04,
        );
        let row_gap = if vs { 8.0 } else { 5.0 };
        let (name_y, state_y) = if two_rows {
            let total = name.size().y + row_gap + state_h;
            let top = r.center().y - total / 2.0;
            (top, top + name.size().y + row_gap)
        } else {
            (r.center().y - name.size().y / 2.0, r.center().y - state_h / 2.0)
        };
        let nh = name.size().y;
        let nw = name.size().x;
        painter.galley(pos2(x0, name_y), name, cream());
        // Tags sit after the name on its row; in a grid they take the right.
        let mut tx = if two_rows { right - tags_w + 6.0 } else { x0 + nw + 12.0 };
        for (g, c) in tag_galleys {
            let tw = g.size().x + 16.0;
            let th = g.size().y + 6.0;
            let tr = Rect::from_min_size(pos2(tx, name_y + nh / 2.0 - th / 2.0), vec2(tw, th));
            chip(painter, tr, 0.1, c);
            painter.galley(pos2(tr.left() + 8.0, tr.top() + 3.0), g, ink(1.0));
            tx += tw + 6.0;
        }
        let sx = if two_rows { x0 } else { right - state_w };
        let sr = Rect::from_min_size(pos2(sx, state_y), vec2(state_w, state_h));
        if state_chip {
            if ready {
                chip(painter, sr, 0.08, go);
            } else {
                let cut = sr.width() * 0.08;
                painter.add(Shape::closed_line(
                    vec![
                        sr.left_top() + vec2(cut, 0.0),
                        sr.right_top(),
                        sr.right_bottom() - vec2(cut, 0.0),
                        sr.left_bottom(),
                    ],
                    Stroke::new(1.5, hsla(45.0, 100.0, 95.0, 0.2)),
                ));
            }
        }
        let mut lx = sr.left() + if state_chip { chip_px } else { 0.0 };
        let sy = sr.center().y;
        if ready {
            let k = state_size * 0.9;
            let stroke = Stroke::new(state_size * 0.16, state_color);
            painter.line_segment([pos2(lx, sy), pos2(lx + k * 0.35, sy + k * 0.32)], stroke);
            painter.line_segment(
                [pos2(lx + k * 0.35, sy + k * 0.32), pos2(lx + k * 0.9, sy - k * 0.38)],
                stroke,
            );
            lx += check;
        }
        let sgh = state_g.size().y;
        painter.galley(pos2(lx, sy - sgh / 2.0), state_g, state_color);
    }

    /// `match-lobby__vote`: the head, and a card per map.
    #[allow(clippy::too_many_arguments)]
    fn vote(
        &mut self,
        ui: &mut egui::Ui,
        painter: &Painter,
        r: Rect,
        top: f32,
        maps_h: f32,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
    ) {
        let title = text(painter, "VOTE MAP", 20.0, style.display.clone(), cream(), 20.0 * 0.14);
        let tw = title.size().x;
        let th = title.size().y;
        painter.galley(pos2(r.left(), top), title, cream());
        let hint = text(
            painter,
            "MOST VOTES WINS · TIES ARE RANDOM",
            13.0,
            style.body.clone(),
            hsla(45.0, 100.0, 95.0, 0.48),
            13.0 * 0.16,
        );
        painter.galley(
            pos2(r.left() + tw + 18.0, top + th - hint.size().y - 1.0),
            hint,
            cream(),
        );
        let maps_top = top + 20.0 + 12.0;
        let slots = self.maps.len().max(3);
        let (cols, rows) = if style.narrower {
            (2, slots.div_ceil(2))
        } else {
            (slots, 1)
        };
        let gap = 26.0;
        let w = (r.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = (maps_h - (rows as f32 - 1.0) * gap) / rows as f32;
        let most = self.maps.iter().map(|m| m.votes).max().unwrap_or(0);
        let leaders = self.maps.iter().filter(|m| m.votes == most).count();
        let starting = self.phase == Phase::Starting;
        let mut vote = None;
        for i in 0..slots {
            let (row, col) = (i / cols, i % cols);
            let depth = 7.8;
            let card = Rect::from_min_size(
                pos2(r.left() + col as f32 * (w + gap), maps_top + row as f32 * (h + gap)),
                vec2(w - depth * 0.75, h - depth - 8.0),
            );
            let Some(map) = self.maps.get(i) else {
                map_slot_empty(painter, card, style);
                continue;
            };
            let leading = map.votes > 0 && map.votes == most && leaders == 1;
            if map_card(ui, painter, card, map, leading, starting, style) {
                vote = Some(map.name.clone());
            }
        }
        if let Some(name) = vote {
            self.select_map_vote(&name, client);
        }
    }

    /// `match-lobby__actions`: Leave, Add bot, Switch and Shuffle, Rejoin, Ready.
    #[allow(clippy::too_many_arguments)]
    fn actions(
        &mut self,
        ui: &mut egui::Ui,
        painter: &Painter,
        rect: Rect,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
        s: &Sizes,
        time: f64,
    ) -> Option<RoomAction> {
        let mut action = None;
        let gap = clamp_css(12.0, style.window_width * 0.011, 20.0);
        let label_size = clamp_css(14.0, style.window_width * 0.01, 18.0);
        let pad = (
            clamp_css(20.0, style.window_width * 0.018, 34.0),
            clamp_css(18.0, style.window_width * 0.017, 32.0),
        );
        let act_depth = 7.2;
        let bottom = rect.bottom() - 4.0;
        let act_top = bottom - act_depth - s.act_h;
        let mut x = rect.left();
        let starting = self.phase == Phase::Starting;
        let act = |ui: &mut egui::Ui,
                   x: &mut f32,
                   id: &str,
                   label: &str,
                   hint: &str,
                   look: KeyLook,
                   hover: Option<(KeyLook, f32)>,
                   disabled: bool,
                   lead: f32| {
            let lg = text(
                painter,
                &label.to_uppercase(),
                label_size,
                style.display.clone(),
                Color32::WHITE,
                label_size * 0.07,
            );
            let hg = text(
                painter,
                &hint.to_uppercase(),
                12.0,
                style.body.clone(),
                Color32::WHITE,
                1.4,
            );
            let w = (lg.size().x + lead).max(hg.size().x) + pad.0 + pad.1 + 8.0;
            let r = Rect::from_min_size(pos2(*x, act_top), vec2(w, s.act_h));
            let mut key = RaisedKey::new(("room-act", id), r, look).disabled(disabled);
            if let Some((h, p)) = hover {
                key = key.hover(h, p);
            }
            let face = key.show(ui);
            // The label stands `lead` in, past a drawn mark; the hint does not.
            KeyText::new(label, "", style.display.clone(), style.body.clone())
                .size(label_size, 12.0)
                .pad(pad.0 + lead, pad.1)
                .layout(KeyTextLayout::Stacked)
                .paint(painter, &shift_face(&face, -(hg.size().y + 6.0) / 2.0));
            KeyText::new(" ", hint, style.display.clone(), style.body.clone())
                .size(label_size, 12.0)
                .pad(pad.0, pad.1)
                .layout(KeyTextLayout::Stacked)
                .paint(painter, &face);
            *x += w + act_depth * 0.75 + 2.0 + gap;
            face
        };

        // Leave, in the danger red.
        let leave_look = KeyLook {
            text: hsl(356.0, 88.0, 56.0),
            sub: hsla(356.0, 70.0, 38.0, 0.7),
            ..KeyLook::cream().depth(act_depth)
        };
        let leave_hover = KeyLook {
            face: vec![
                (0.0, hsl(356.0, 95.0, 68.0)),
                (0.55, hsl(356.0, 85.0, 54.0)),
                (1.0, hsl(352.0, 80.0, 44.0)),
            ],
            side: [hsl(350.0, 75.0, 30.0), hsl(345.0, 72.0, 18.0)],
            shadow: hsla(350.0, 80.0, 14.0, 0.5),
            text: hsl(45.0, 100.0, 96.0),
            sub: hsla(45.0, 100.0, 96.0, 0.72),
            ..leave_look.clone()
        };
        let arrow = label_size * 0.9;
        let leave = act(
            ui,
            &mut x,
            "leave",
            "Leave",
            "Back to play",
            leave_look,
            Some((leave_hover, -0.35)),
            false,
            arrow,
        );
        // `←`, drawn: the face's font may not have the arrow.
        {
            let hint_h = 12.0 * 1.2;
            let y = leave.rect.center().y - (hint_h + 6.0) / 2.0;
            let ax = leave.rect.left() + pad.0;
            let stroke = Stroke::new(2.4, leave.text);
            let k = label_size * 0.42;
            painter.line_segment([pos2(ax, y), pos2(ax + k * 1.4, y)], stroke);
            painter.line_segment([pos2(ax, y), pos2(ax + k * 0.6, y - k * 0.55)], stroke);
            painter.line_segment([pos2(ax, y), pos2(ax + k * 0.6, y + k * 0.55)], stroke);
        }
        if leave.clicked {
            client.leave_lobby(&self.lobby_id);
            action = Some(RoomAction::Leave);
        }

        let full = self.players.len() >= self.max_players;
        let bot_disabled = starting || self.invite_bot_pending.is_some() || full || self.bot_note.is_some();
        let cream_act = KeyLook::cream().depth(act_depth);
        let bot_hint = self.bot_hint();
        let bot = act(
            ui,
            &mut x,
            "bot",
            "+ Add bot",
            &bot_hint,
            cream_act.clone(),
            None,
            bot_disabled,
            0.0,
        );
        if bot.clicked {
            self.request_bot(client);
        }
        let flash_at = pos2(x - gap + 14.0, bot.rect.center().y);

        // Two seats facing each other have nothing to switch or shuffle.
        if self.has_teams && self.max_players > 2 {
            let to = if self.local_team() == Some("alpha") {
                "bravo"
            } else {
                "alpha"
            };
            let hint = format!("To {}", self.team_name(to));
            let switch = act(
                ui,
                &mut x,
                "switch",
                "Switch",
                &hint,
                cream_act.clone(),
                None,
                starting,
                0.0,
            );
            if switch.clicked {
                client.switch_team(&self.lobby_id);
            }
            let hint = if self.shuffle_sent {
                "Vote sent"
            } else {
                "Mix the teams"
            };
            let shuffle = act(
                ui,
                &mut x,
                "shuffle",
                "Shuffle",
                hint,
                cream_act.clone(),
                None,
                starting || self.shuffle_sent,
                0.0,
            );
            if shuffle.clicked {
                client.vote_shuffle(&self.lobby_id);
                self.shuffle_sent = true;
            }
        }

        // Ready, at the right.
        let (ready_label, ready_hint) = self.ready_text();
        let ready_size = if style.short {
            clamp_css(22.0, style.window_width * 0.017, 30.0)
        } else {
            clamp_css(26.0, style.window_width * 0.019, 36.0)
        };
        let ready_depth = 10.2;
        let ready_w = clamp_css(200.0, style.window_width * 0.17, 330.0);
        let ready_rect = Rect::from_min_size(
            pos2(
                rect.right() - ready_w - ready_depth * 0.75 - 4.0,
                bottom - ready_depth - s.ready_h,
            ),
            vec2(ready_w, s.ready_h),
        );
        let a = style.accent;
        let calling = self.enough_to_start() && !self.local_ready && !starting;
        if calling {
            // `match-lobby__ready--calling`: a glow that breathes behind it.
            let breath = 0.5 - 0.5 * (time * std::f64::consts::TAU / 1.1).cos() as f32;
            for grow in [14.0, 9.0, 4.0] {
                let g = ready_rect.expand2(vec2(grow, grow * 0.7));
                let cut = g.width() * 0.05;
                painter.add(Shape::convex_polygon(
                    vec![
                        g.left_top() + vec2(cut, 0.0),
                        g.right_top(),
                        g.right_bottom() - vec2(cut, 0.0),
                        g.left_bottom(),
                    ],
                    a.face.gamma_multiply(0.25 * breath),
                    Stroke::NONE,
                ));
            }
        }
        let ready_look = KeyLook {
            face: vec![(0.0, a.hi), (0.58, a.face), (1.0, a.lo)],
            side: [a.side_hi, a.side_lo],
            shadow: hsla(250.0, 70.0, 3.0, 0.62),
            sub: hsla(250.0, 45.0, 8.0, 0.62),
            depth: ready_depth,
            cut: 0.05,
            ..KeyLook::gold()
        };
        let held_look = KeyLook {
            face: vec![
                (0.0, hsl(250.0, 28.0, 21.0)),
                (0.6, hsl(250.0, 40.0, 10.0)),
                (1.0, hsl(252.0, 44.0, 6.0)),
            ],
            side: [hsl(252.0, 44.0, 5.0), hsl(252.0, 44.0, 3.0)],
            text: a.hi,
            sub: a.hi.gamma_multiply(0.66),
            ..ready_look.clone()
        };
        let mut ready_key = if self.local_ready {
            RaisedKey::new(("room-ready", &self.lobby_id), ready_rect, held_look.clone())
                .rest_press(0.62)
                .hover(held_look, 0.62)
        } else {
            RaisedKey::new(("room-ready", &self.lobby_id), ready_rect, ready_look)
        };
        ready_key = ready_key.disabled(starting);
        let face = ready_key.show(ui);
        let label = text(
            painter,
            &ready_label.to_uppercase(),
            ready_size,
            style.display.clone(),
            face.text,
            ready_size * 0.12,
        );
        let hint = text(
            painter,
            &ready_hint.to_uppercase(),
            13.0,
            style.body.clone(),
            face.sub,
            13.0 * 0.12,
        );
        let total = label.size().y + 8.0 + hint.size().y;
        let ty = face.rect.center().y - total / 2.0;
        let lh = label.size().y;
        painter.galley(pos2(face.rect.center().x - label.size().x / 2.0, ty), label, face.text);
        painter.galley(
            pos2(face.rect.center().x - hint.size().x / 2.0, ty + lh + 8.0),
            hint,
            face.sub,
        );
        if face.clicked {
            let ready = !self.local_ready;
            self.set_ready(ready, client);
        }

        // Rejoin, left of Ready, while this room's match runs.
        if let Some(match_id) = self.active_match_id.clone() {
            let hint = self.recover_note.as_deref().unwrap_or("Unplayed seat").to_owned();
            let lg = text(
                painter,
                "REJOIN MATCH",
                label_size,
                style.display.clone(),
                Color32::WHITE,
                label_size * 0.07,
            );
            let hg = text(
                painter,
                &hint.to_uppercase(),
                12.0,
                style.body.clone(),
                Color32::WHITE,
                1.4,
            );
            let w = lg.size().x.max(hg.size().x) + pad.0 + pad.1 + 8.0;
            let rx = ready_rect.left() - gap - act_depth * 0.75 - 2.0 - w;
            if rx > x {
                let mut rx_cursor = rx;
                let rejoin = act(
                    ui,
                    &mut rx_cursor,
                    "rejoin",
                    "Rejoin match",
                    &hint,
                    cream_act,
                    None,
                    !style.online,
                    0.0,
                );
                if rejoin.clicked {
                    self.recover_note = Some("Checking seat…".into());
                    action = Some(RoomAction::Rejoin(match_id));
                }
            }
        }

        // A note beside Add bot that fades by itself.
        if let Some((note, at)) = &self.flash {
            let t = at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
            let alpha = if t < 0.1 {
                t / 0.1
            } else if t < 0.7 {
                1.0
            } else {
                (1.0 - t) / 0.3
            };
            let g = text(
                painter,
                &note.to_uppercase(),
                15.0,
                style.display.clone(),
                ink(alpha),
                1.5,
            );
            let slide = if t < 0.1 { -8.0 * (1.0 - t / 0.1) } else { 0.0 };
            let r = Rect::from_min_size(
                pos2(flash_at.x + slide, flash_at.y - (g.size().y + 24.0) / 2.0),
                vec2(g.size().x + 44.0, g.size().y + 24.0),
            );
            let shape = vec![
                r.left_top() + vec2(10.0, 0.0),
                r.right_top(),
                r.right_bottom() - vec2(10.0, 0.0),
                r.left_bottom(),
            ];
            let shadow: Vec<Pos2> = shape.iter().map(|p| *p + vec2(6.0, 6.0)).collect();
            painter.add(Shape::convex_polygon(
                shadow,
                hsla(250.0, 45.0, 4.0, 0.6 * alpha),
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                shape,
                cream().gamma_multiply(alpha),
                Stroke::NONE,
            ));
            painter.galley(pos2(r.left() + 22.0, r.top() + 12.0), g, ink(alpha));
        }
        action
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

/// The CSS's sizes at this window size.
struct Sizes {
    gap: f32,
    act_h: f32,
    ready_h: f32,
    ffa_seat_h: f32,
    vs_seat_h: f32,
}

impl Sizes {
    fn new(style: &RoomStyle<'_>) -> Self {
        Self {
            gap: if style.short { 14.0 } else { 20.0 },
            act_h: if style.short { 56.0 } else { 66.0 },
            ready_h: if style.short { 76.0 } else { 100.0 },
            ffa_seat_h: if style.short { 50.0 } else { 62.0 },
            vs_seat_h: if style.short {
                80.0
            } else if style.narrow {
                96.0
            } else {
                116.0
            },
        }
    }
}

/// The same face moved down by `dy`, for lettering set off its centre.
fn shift_face(face: &KeyFace, dy: f32) -> KeyFace {
    KeyFace {
        rect: face.rect.translate(vec2(0.0, dy)),
        ..face.clone()
    }
}

/// `--ink`.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 7.0, a)
}

/// The invite: the server's share link on the game's site.
#[must_use]
pub fn invite_href(site: &str, share_url: &str, lobby_id: &str) -> String {
    let share = if share_url.is_empty() {
        format!("/?lobby={lobby_id}")
    } else {
        share_url.to_owned()
    };
    if share.contains("://") {
        return share;
    }
    let site = site.trim_end_matches('/');
    let path = if share.starts_with('/') {
        share
    } else {
        format!("/{share}")
    };
    format!("{site}{path}")
}

fn ease_in_out(t: f32) -> f32 {
    // CSS `ease-in-out`, close enough for a light band.
    t * t * (3.0 - 2.0 * t)
}

/// A rectangle filled left to right through `stops`.
fn gradient_rect_x(rect: Rect, stops: &[(f32, Color32)]) -> Mesh {
    let mut mesh = Mesh::default();
    for (i, (at, color)) in stops.iter().enumerate() {
        let x = rect.min.x + at * rect.width();
        mesh.colored_vertex(pos2(x, rect.min.y), *color);
        mesh.colored_vertex(pos2(x, rect.max.y), *color);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b + 1);
            mesh.add_triangle(b - 2, b + 1, b);
        }
    }
    mesh
}

/// A band across `rect`: transparent outside the stops.
fn horizontal_band(painter: &Painter, rect: Rect, stops: &[(f32, Color32)]) {
    painter.add(gradient_rect_x(rect, stops));
}

/// A slanted chip (`clip-path: polygon(cut 0, 100% 0, 100%-cut 100%, 0 100%)`).
fn chip(painter: &Painter, r: Rect, cut: f32, color: Color32) {
    let c = r.width() * cut;
    painter.add(Shape::convex_polygon(
        vec![
            r.left_top() + vec2(c, 0.0),
            r.right_top(),
            r.right_bottom() - vec2(c, 0.0),
            r.left_bottom(),
        ],
        color,
        Stroke::NONE,
    ));
}

fn vs_galley(painter: &Painter, size: f32, style: &RoomStyle<'_>) -> std::sync::Arc<egui::Galley> {
    text(
        painter,
        "VS",
        size,
        style.display.clone(),
        hsl(46.0, 100.0, 60.0),
        -size * 0.02,
    )
}

/// `match-lobby__vs`: gold, skewed, on a hard shadow.
fn paint_vs(painter: &Painter, centre: Pos2, g: std::sync::Arc<egui::Galley>) {
    let at = centre - g.size() / 2.0;
    let size = g.size().y;
    painter.galley(at + vec2(size * 0.08, size * 0.1), g.clone(), hsl(250.0, 50.0, 3.0));
    painter.galley(at, g, hsl(46.0, 100.0, 60.0));
}

/// The face's part from `y0` to `y1`, for a key cut by `cut` px.
fn face_band(face: Rect, cut: f32, y0: f32, y1: f32) -> [Pos2; 4] {
    let h = face.height();
    let left = |y: f32| face.left() + cut * (1.0 - (y - face.top()) / h);
    let right = |y: f32| face.right() - cut * (y - face.top()) / h;
    [
        pos2(left(y0), y0),
        pos2(right(y0), y0),
        pos2(right(y1), y1),
        pos2(left(y1), y1),
    ]
}

/// A polygon showing `texture` as `background-size: cover` over `area`.
fn textured_polygon(painter: &Painter, points: &[Pos2], texture: TextureId, area: Rect, image: [f32; 2]) {
    // Cover: crop the image to the area's aspect.
    let (iw, ih) = (image[0].max(1.0), image[1].max(1.0));
    let scale = (area.width() / iw).max(area.height() / ih);
    let shown = vec2(area.width() / (iw * scale), area.height() / (ih * scale));
    let uv0 = pos2((1.0 - shown.x) / 2.0, (1.0 - shown.y) / 2.0);
    let mut mesh = Mesh::with_texture(texture);
    for p in points {
        let t = vec2((p.x - area.left()) / area.width(), (p.y - area.top()) / area.height());
        mesh.vertices.push(Vertex {
            pos: *p,
            uv: pos2(uv0.x + t.x * shown.x, uv0.y + t.y * shown.y),
            color: Color32::WHITE,
        });
    }
    for i in 1..points.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    painter.add(mesh);
}

/// A map's card (`raised-key--map`); true when the player votes for it.
fn map_card(
    ui: &mut egui::Ui,
    painter: &Painter,
    rect: Rect,
    map: &MatchLobbyMapOption,
    leading: bool,
    starting: bool,
    style: &RoomStyle<'_>,
) -> bool {
    let a = style.accent;
    let rest = KeyLook {
        face: vec![
            (0.0, hsl(45.0, 100.0, 97.0)),
            (0.62, hsl(42.0, 58.0, 89.0)),
            (1.0, hsl(40.0, 40.0, 80.0)),
        ],
        side: [hsl(36.0, 24.0, 56.0), hsl(30.0, 20.0, 38.0)],
        shadow: hsla(250.0, 70.0, 2.0, 0.6),
        depth: 7.8,
        cut: 0.024,
        ..KeyLook::cream()
    };
    let picked = KeyLook {
        face: vec![(0.0, a.hi), (0.58, a.face), (1.0, a.lo)],
        side: [a.side_hi, a.side_lo],
        text: hsl(250.0, 45.0, 8.0),
        sub: hsla(250.0, 45.0, 8.0, 0.66),
        ..rest.clone()
    };
    let key = if map.voted_by_local {
        RaisedKey::new(("room-map", map.name.as_str()), rect, picked.clone())
            .rest_press(-0.5)
            .hover(picked, -0.75)
            .active_face(vec![(0.0, a.face), (1.0, a.lo)])
    } else {
        RaisedKey::new(("room-map", map.name.as_str()), rect, rest)
    };
    let face: KeyFace = key.disabled(starting).show(ui);
    let f = face.rect;
    let cut = f.width() * 0.024;
    let name_size = if style.narrow {
        14.0
    } else {
        clamp_css(14.0, style.window_width * 0.0115, 20.0)
    };
    let id_size = if style.narrow { 11.0 } else { 13.0 };
    let (pad_t, pad_b) = if style.narrow { (11.0, 13.0) } else { (13.0, 15.0) };
    let info_h = name_size + pad_t + pad_b;
    let shot_bottom = f.bottom() - info_h;
    if shot_bottom - f.top() > 20.0 {
        let band = face_band(f, cut, f.top(), shot_bottom);
        gradient_polygon(
            painter,
            &band,
            &[(0.0, hsl(250.0, 30.0, 18.0)), (1.0, hsl(262.0, 34.0, 12.0))],
        );
        let area = Rect::from_min_max(f.min, pos2(f.right(), shot_bottom));
        if let Some(tex) = (style.map_shot)(&map.name) {
            let size = ui
                .ctx()
                .tex_manager()
                .read()
                .meta(tex)
                .map_or([16.0, 9.0], |m| [m.size[0] as f32, m.size[1] as f32]);
            // Grows a little under the pointer, as the browser's does.
            let zoom = if face.hovered { 1.06 } else { 1.0 };
            let zoomed = Rect::from_center_size(area.center(), area.size() * zoom);
            let dim = if starting { 0.6 } else { 1.0 };
            let mut mesh_painter = painter.clone();
            mesh_painter.set_opacity(dim);
            textured_polygon(&mesh_painter, &band, tex, zoomed, size);
        }
        let shade = face_band(f, cut, f.top() + (shot_bottom - f.top()) * 0.55, shot_bottom);
        gradient_polygon(
            painter,
            &shade,
            &[(0.0, Color32::TRANSPARENT), (1.0, hsla(250.0, 50.0, 4.0, 0.55))],
        );
        // Flags, top left.
        let mut fx = f.left() + 22.0;
        let mut flags = Vec::new();
        if map.voted_by_local {
            flags.push(("YOUR VOTE", ink(1.0), a.face));
        }
        if leading {
            flags.push(("LEADING", ink(1.0), hsl(96.0, 85.0, 55.0)));
        }
        for (label, fg, bg) in flags {
            let g = text(painter, label, 11.0, style.display.clone(), fg, 11.0 * 0.12);
            let r = Rect::from_min_size(pos2(fx, f.top() + 12.0), vec2(g.size().x + 24.0, g.size().y + 12.0));
            chip(painter, r, 0.08, bg);
            painter.galley(pos2(r.left() + 12.0, r.top() + 6.0), g, fg);
            fx = r.right() + 8.0;
        }
        // Votes, bottom right.
        if map.votes > 0 {
            let n = text(
                painter,
                &map.votes.to_string(),
                46.0,
                style.display.clone(),
                cream(),
                0.0,
            );
            let word = if map.votes == 1 { "vote" } else { "votes" };
            let w = text(
                painter,
                word,
                14.0,
                style.display.clone(),
                hsla(45.0, 100.0, 95.0, 0.7),
                14.0 * 0.14,
            );
            let total = n.size().x + 8.0 + w.size().x;
            let x = f.right() - 18.0 - total;
            let y = shot_bottom - 8.0 - n.size().y;
            let nh = n.size().y;
            let nw = n.size().x;
            painter.galley(pos2(x + 3.0, y + 4.0), n.clone(), hsl(250.0, 50.0, 3.0));
            painter.galley(pos2(x, y), n, cream());
            painter.galley(pos2(x + nw + 8.0, y + nh * 0.78 - w.size().y), w, cream());
        }
    }
    // The name and id under the shot.
    let id = text(
        painter,
        &map.name.to_uppercase(),
        id_size,
        style.body.clone(),
        face.sub,
        id_size * 0.14,
    );
    let ix = f.right() - cut - 26.0 - id.size().x;
    let cy = shot_bottom + info_h / 2.0;
    let idh = id.size().y;
    painter.galley(pos2(ix, cy - idh / 2.0), id, face.sub);
    let title = if map.display_name.is_empty() {
        &map.name
    } else {
        &map.display_name
    };
    let room = ix - 12.0 - (f.left() + 22.0);
    let shown = elide(
        painter,
        &title.to_uppercase(),
        name_size,
        style.display.clone(),
        room.max(20.0),
    );
    let name = text(
        painter,
        &shown,
        name_size,
        style.display.clone(),
        face.text,
        name_size * 0.04,
    );
    painter.galley(pos2(f.left() + 22.0, cy - name.size().y / 2.0), name, face.text);
    face.clicked
}

/// A slot without a map (`raised-key--map-empty`).
fn map_slot_empty(painter: &Painter, rect: Rect, style: &RoomStyle<'_>) {
    let look = KeyLook {
        face: vec![(0.0, hsl(250.0, 23.0, 16.0)), (1.0, hsl(250.0, 23.0, 16.0))],
        side: [hsl(250.0, 20.0, 11.0), hsl(250.0, 20.0, 11.0)],
        shadow: hsla(250.0, 70.0, 2.0, 0.6),
        depth: 7.8,
        cut: 0.024,
        ..KeyLook::cream()
    };
    let f = super::raised_key::paint_key(painter, rect, &look, 0.0, 0.55);
    let dash = text(
        painter,
        "—",
        64.0,
        style.display.clone(),
        hsla(250.0, 15.0, 80.0, 0.25),
        0.0,
    );
    painter.galley(f.center() - dash.size() / 2.0 - vec2(0.0, 18.0), dash, cream());
    let name = text(
        painter,
        "UNAVAILABLE",
        14.0,
        style.display.clone(),
        hsla(45.0, 30.0, 90.0, 0.55 * 0.55),
        0.6,
    );
    painter.galley(pos2(f.left() + 22.0, f.bottom() - 15.0 - name.size().y), name, cream());
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
    fn quick_play_adds_its_bots_one_at_a_time_then_readies() {
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
        assert!(view.local_ready);
        assert_eq!(view.status(), "0 players still to ready up.");
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
