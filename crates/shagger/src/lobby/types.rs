//! SHAG `src/lobby/types.ts`: the lobby's JSON messages.
//!
//! Server messages the client acts on are typed; every other message
//! (statistics, leaderboards, settings, room updates, development tools)
//! arrives as [`LobbyIn::Other`] with its JSON, so a lobby that adds a message
//! never breaks the client. Field names follow the TypeScript (camelCase).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `RatingPool`: a rating pool, one lobby visibility or Overall, which rates
/// every match. A stats or leaderboard request that leaves it out reads
/// Overall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RatingPool {
    Public,
    Private,
    Overall,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamNames {
    pub alpha: String,
    pub bravo: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GametypeInfo {
    /// `duel`, `snd`, `ffa`, ...
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub max_players: u32,
    pub min_players: Option<u32>,
    pub has_teams: bool,
    pub team_names: Option<TeamNames>,
    #[serde(default)]
    pub map_pool: Vec<String>,
    pub background_image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerInfo {
    pub id: String,
    pub name: String,
    /// `online`, `in_queue`, `in_lobby` or `in_match`.
    pub status: String,
    pub lobby_id: Option<String>,
    pub match_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub from: String,
    pub from_id: String,
    pub text: String,
    pub ts: f64,
}

/// A match visible to every client.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMatchInfo {
    /// The lobby (room) id.
    pub id: String,
    pub gametype: String,
    pub gametype_name: String,
    /// Empty until the match starts.
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub map_display_name: String,
    #[serde(default)]
    pub players: Vec<String>,
    pub player_count: u32,
    pub max_players: u32,
    /// `waiting` or `playing`.
    pub state: String,
    pub started_at: f64,
    /// The game server, once playing, for spectators.
    pub server_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PlayerIdentityInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    /// Older servers omit this; keep private rooms off then.
    pub features: Option<Value>,
    /// This connection's id, used by rooms and chat. Not the account id.
    pub player_id: String,
    /// The account this connection is logged in to; absent for bots.
    pub identity: Option<PlayerIdentityInfo>,
    /// Only when this login created the account: its key, sent this once. Store it.
    pub key: Option<String>,
    #[serde(default)]
    pub gametypes: Vec<GametypeInfo>,
    #[serde(default)]
    pub online_players: Vec<PlayerInfo>,
    #[serde(default)]
    pub chat_history: Vec<ChatMessage>,
    #[serde(default)]
    pub live_matches: Vec<LiveMatchInfo>,
    #[serde(default)]
    pub recent_demos: Vec<Value>,
}

/// Login failed. The connection stays open except after `lobby_full`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthError {
    /// `unknown_key`, `rate_limited`, `already_authenticated` or `lobby_full`.
    pub code: String,
    pub online: Option<u32>,
    pub max_players: Option<u32>,
    pub retry_after_sec: Option<u32>,
}

/// Go to this game server; the ticket is this player's single-use seat.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyConnect {
    pub match_id: Option<String>,
    pub server_url: String,
    pub map: String,
    pub gametype: String,
    pub admission_ticket: Option<String>,
}

/// A message from the lobby.
#[derive(Debug, Clone, PartialEq)]
pub enum LobbyIn {
    Welcome(Box<Welcome>),
    AuthError(AuthError),
    MatchesUpdate(Vec<LiveMatchInfo>),
    Connect(LobbyConnect),
    Chat(ChatMessage),
    /// `lobby:created`, which is also the full room state on every join.
    RoomState(Value),
    LobbyError(String),
    /// Any other message, by its `type`.
    Other {
        kind: String,
        message: Value,
    },
}

impl LobbyIn {
    /// Reads one text frame. `None` when it is not a JSON object with a
    /// string `type` (the browser logs and drops those).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let message: Value = serde_json::from_str(text).ok()?;
        let kind = message.get("type")?.as_str()?.to_owned();
        let typed = |m: &Value| -> Option<Self> {
            Some(match kind.as_str() {
                "welcome" => Self::Welcome(Box::new(serde_json::from_value(m.clone()).ok()?)),
                "auth:error" => Self::AuthError(serde_json::from_value(m.clone()).ok()?),
                "matches:update" => Self::MatchesUpdate(serde_json::from_value(m.get("matches")?.clone()).ok()?),
                "lobby:connect" => Self::Connect(serde_json::from_value(m.clone()).ok()?),
                "chat:message" => Self::Chat(serde_json::from_value(m.clone()).ok()?),
                "lobby:created" => Self::RoomState(m.get("lobby")?.clone()),
                "lobby:error" => Self::LobbyError(m.get("error")?.as_str()?.to_owned()),
                _ => return None,
            })
        };
        Some(typed(&message).unwrap_or(Self::Other { kind, message }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_messages_the_client_acts_on() {
        let w = LobbyIn::parse(
            r#"{"type":"welcome","playerId":"c7","identity":{"id":"a1","name":"Player_12"},"key":"k",
                "gametypes":[{"id":"duel","name":"DUEL","description":"","maxPlayers":2,"hasTeams":false,"mapPool":["washoi_dm2"]}],
                "onlinePlayers":[],"chatHistory":[],"liveMatches":[],"recentDemos":[]}"#,
        );
        let Some(LobbyIn::Welcome(w)) = w else { panic!("{w:?}") };
        assert_eq!(w.identity.unwrap().name, "Player_12");
        assert_eq!(w.gametypes[0].map_pool, ["washoi_dm2"]);

        let c = LobbyIn::parse(
            r#"{"type":"lobby:connect","serverUrl":"wss://eu.washoi.net:7781","map":"washoi_dm2","gametype":"duel","admissionTicket":"t"}"#,
        );
        assert!(matches!(
            c,
            Some(LobbyIn::Connect(LobbyConnect {
                admission_ticket: Some(_),
                ..
            }))
        ));

        let m = LobbyIn::parse(
            r#"{"type":"matches:update","matches":[{"id":"l1","gametype":"ffa","gametypeName":"FFA","map":"washoi_dm7","mapDisplayName":"DM7","players":["a"],"playerCount":1,"maxPlayers":8,"state":"playing","startedAt":1,"serverUrl":"ws://x:7777"}]}"#,
        );
        assert!(matches!(&m, Some(LobbyIn::MatchesUpdate(v)) if v[0].server_url.as_deref() == Some("ws://x:7777")));

        // Unknown and malformed-but-typed messages keep their JSON.
        assert!(
            matches!(LobbyIn::parse(r#"{"type":"queue:left"}"#), Some(LobbyIn::Other { kind, .. }) if kind == "queue:left")
        );
        assert!(matches!(
            LobbyIn::parse(r#"{"type":"welcome"}"#),
            Some(LobbyIn::Other { .. })
        ));
        assert_eq!(LobbyIn::parse("[1]"), None);
        assert_eq!(LobbyIn::parse("nope"), None);
    }
}
