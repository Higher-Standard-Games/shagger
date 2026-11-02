//! SHAG `src/engine/ui/components/MatchLobbyView.ts`: one room of the lobby
//! before its match. The players and their ready state, the map vote, adding
//! bots, readying up, and quick play's self-driving fill (`fillWithBots`).
//!
//! The browser builds this from HTML; here the same state draws with egui.
//! The lobby's messages come in through [`MatchLobbyView::handle`], and what
//! the player does goes straight to the [`LobbyClient`].

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::lobby::lobby_client::LobbyClient;
use crate::lobby::types::LobbyIn;

/// How long an unanswered bot invite holds the key down.
const BOT_INVITE_TIMEOUT: Duration = Duration::from_secs(9);

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
    pub maps: Vec<MatchLobbyMapOption>,
    pub players: Vec<MatchLobbyPlayer>,
    pub local_ready: bool,
    pub phase: Phase,
    pub match_in_progress: bool,
    pub room_error: Option<String>,
    active_match_id: Option<String>,
    local_player_id: Option<String>,
    invite_bot_pending: Option<Instant>,
    /// Quick play: bots still to add before this player readies up; 0 when off.
    auto_fill_bots: usize,
    /// A note under the keys, such as "Lobby changed to private".
    flash: Option<(String, Instant)>,
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
            lobby_id: str_of(lobby, "id").to_owned(),
            gametype_label,
            max_players,
            min_players,
            has_teams: config.get("hasTeams").and_then(Value::as_bool).unwrap_or(false),
            team_names: (team("alpha", "Team Soba"), team("bravo", "Team Udon")),
            private_lobby: str_of(lobby, "visibility") == "private",
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
            LobbyIn::Connect(_) => {
                self.room_error = None;
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
                    self.flash = Some(("Lobby changed to private".into(), Instant::now()));
                }
            }
            "lobby:bot_invite_failed" => {
                self.invite_bot_pending = None;
                self.auto_fill_bots = 0;
                let why = str_of(msg, "error");
                self.flash = Some((format!("No bots available ({why})"), Instant::now()));
            }
            "lobby:all_ready" => {
                self.room_error = None;
                self.phase = Phase::Starting;
                for p in &mut self.players {
                    p.ready = true;
                }
            }
            "match:ended" => {
                self.room_error = None;
                self.active_match_id = None;
                self.match_in_progress = false;
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

    /// Per frame: an invite the server never answered lets go.
    pub fn tick(&mut self) {
        if self
            .invite_bot_pending
            .is_some_and(|at| at.elapsed() >= BOT_INVITE_TIMEOUT)
        {
            self.invite_bot_pending = None;
            self.auto_fill_bots = 0;
        }
    }

    pub fn bot_count(&self) -> usize {
        self.players.iter().filter(|p| p.bot_level.is_some()).count()
    }

    fn select_map_vote(&mut self, name: &str, client: &LobbyClient) {
        for map in &mut self.maps {
            map.voted_by_local = map.name == name;
        }
        client.vote_map(&self.lobby_id, name);
    }

    fn set_ready(&mut self, ready: bool, client: &LobbyClient) {
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

    /// The status line under the header.
    #[must_use]
    pub fn status(&self) -> String {
        if let Some(e) = &self.room_error {
            return e.clone();
        }
        if self.phase == Phase::Starting {
            return if self.match_in_progress {
                "Match in progress".into()
            } else {
                "Starting…".into()
            };
        }
        let ready = self.players.iter().filter(|p| p.ready).count();
        if self.players.len() < self.min_players {
            format!("Waiting for players ({}/{})", self.players.len(), self.min_players)
        } else {
            format!("{ready}/{} ready", self.players.len())
        }
    }

    fn team_label(&self, team: &str) -> &str {
        match team {
            "alpha" => &self.team_names.0,
            "bravo" => &self.team_names.1,
            _ => "",
        }
    }

    /// Draws the room; `Some(Leave)` when the player leaves it.
    pub fn ui(&mut self, ui: &mut egui::Ui, client: &LobbyClient) -> Option<RoomAction> {
        self.tick();
        let mut action = None;
        ui.horizontal(|ui| {
            ui.heading(&self.gametype_label);
            ui.label(format!("{}/{}", self.players.len(), self.max_players));
            if self.private_lobby {
                ui.label("· private");
            }
        });
        ui.label(self.status());
        ui.separator();
        egui::Grid::new("match_lobby_players").striped(true).show(ui, |ui| {
            for p in &self.players {
                let name = if p.is_local {
                    egui::RichText::new(&p.name).strong()
                } else {
                    egui::RichText::new(&p.name)
                };
                ui.label(name);
                if self.has_teams {
                    ui.label(self.team_label(&p.team));
                }
                ui.label(p.bot_level.as_deref().map_or(String::new(), |l| format!("bot · {l}")));
                ui.label(if p.ready { "READY" } else { "" });
                ui.end_row();
            }
            for _ in self.players.len()..self.max_players {
                ui.weak("open seat");
                ui.end_row();
            }
        });
        if !self.maps.is_empty() {
            ui.separator();
            ui.label("Map vote");
            let waiting = self.phase == Phase::Waiting;
            let mut vote = None;
            ui.horizontal_wrapped(|ui| {
                for map in &self.maps {
                    let title = if map.display_name.is_empty() {
                        &map.name
                    } else {
                        &map.display_name
                    };
                    let text = format!("{title} ({})", map.votes);
                    if ui
                        .add_enabled(waiting, egui::Button::selectable(map.voted_by_local, text))
                        .clicked()
                    {
                        vote = Some(map.name.clone());
                    }
                }
            });
            if let Some(name) = vote {
                self.select_map_vote(&name, client);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let can_add = self.phase == Phase::Waiting
                && self.invite_bot_pending.is_none()
                && self.players.len() < self.max_players;
            let add = if self.invite_bot_pending.is_some() {
                "Summoning…"
            } else {
                "+ Add bot"
            };
            if ui.add_enabled(can_add, egui::Button::new(add)).clicked() {
                self.request_bot(client);
            }
            let ready = if self.local_ready { "Unready" } else { "Ready" };
            if ui
                .add_enabled(self.phase == Phase::Waiting, egui::Button::new(ready))
                .clicked()
            {
                self.set_ready(!self.local_ready, client);
            }
            if ui.button("Leave").clicked() {
                client.leave_lobby(&self.lobby_id);
                action = Some(RoomAction::Leave);
            }
        });
        if let Some((note, at)) = &self.flash {
            if at.elapsed() < Duration::from_secs(3) {
                ui.label(note);
            } else {
                self.flash = None;
            }
        }
        action
    }
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
        assert_eq!(view.status(), "3/3 ready");
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
}
