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

// ── Campaign ──

/// `CampaignModels`: the models a campaign match draws each side with, by
/// model id.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct CampaignModels {
    pub ally: String,
    pub enemy: String,
}

/// `CampaignBotInfo`: one of a mission's bots.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignBotInfo {
    pub name: String,
    /// Bot preset: easy, medium, hard, insane.
    #[serde(default)]
    pub difficulty: String,
    /// `ally` or `enemy`; free-for-all bots are all enemies.
    #[serde(default)]
    pub side: String,
    /// This bot's model, when it is not its side's (a mixed crew).
    pub model: Option<String>,
    /// How much bigger than a player this bot is drawn and hit; absent is 1.
    pub scale: Option<f64>,
}

/// `CampaignMissionInfo`: one operation, as the lobby runs it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignMissionInfo {
    /// Stable id; progress is stored by it.
    pub id: String,
    /// Play order, from 1.
    pub number: u32,
    pub map: String,
    #[serde(default)]
    pub map_title: String,
    pub gametype: String,
    #[serde(default)]
    pub bots: Vec<CampaignBotInfo>,
    #[serde(default)]
    pub models: CampaignModels,
    /// S&D: `attack`, the players attack every round.
    pub side: Option<String>,
}

/// `CampaignClear`: one mission an account has cleared, with its best clear.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignClear {
    pub mission_id: String,
    /// First clear, ms since the epoch.
    #[serde(default)]
    pub cleared_at: f64,
    /// Shortest winning match, in ms.
    #[serde(default)]
    pub best_time_ms: f64,
    /// Most frags in a winning match.
    #[serde(default)]
    pub best_frags: i64,
}

/// `CampaignProgress` (`campaign:state`): the campaign and an account's
/// progress in it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignProgress {
    /// Missions in all, written or not; the ones past `missions` are not
    /// playable yet.
    #[serde(default)]
    pub length: u32,
    #[serde(default)]
    pub missions: Vec<CampaignMissionInfo>,
    #[serde(default)]
    pub clears: Vec<CampaignClear>,
    /// The highest mission number this account may play.
    #[serde(default)]
    pub unlocked_through: u32,
    /// When this account first finished every mission.
    pub completed_at: Option<f64>,
}

/// `LobbyCampaignInfo`: what a campaign room is playing.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyCampaignInfo {
    pub mission_id: String,
    #[serde(default)]
    pub models: CampaignModels,
    /// Bots drawn with a model of their own, by name.
    #[serde(default)]
    pub bot_models: std::collections::BTreeMap<String, String>,
    /// Bots drawn bigger than a player, by name.
    #[serde(default)]
    pub bot_scales: std::collections::BTreeMap<String, f64>,
    /// Bots slime and lava do not hurt, by name.
    #[serde(default)]
    pub slime_immune: Vec<String>,
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
    /// A campaign match: its mission and the models to draw.
    #[serde(default)]
    pub campaign: Option<Box<LobbyCampaignInfo>>,
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
    /// `campaign:state`: the campaign and this account's progress.
    CampaignState(Box<CampaignProgress>),
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
                "campaign:state" => Self::CampaignState(Box::new(serde_json::from_value(m.clone()).ok()?)),
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

        let campaign = LobbyIn::parse(
            r#"{"type":"lobby:connect","serverUrl":"ws://x:7777","map":"washoi_amphi2","gametype":"amphi",
                "campaign":{"missionId":"cartel-contract","models":{"ally":"robot","enemy":"cartel"},"botScales":{"Boss":1.6}}}"#,
        );
        let Some(LobbyIn::Connect(c)) = campaign else {
            panic!("{campaign:?}");
        };
        let info = c.campaign.unwrap();
        assert_eq!(
            (info.mission_id.as_str(), info.models.enemy.as_str()),
            ("cartel-contract", "cartel")
        );
        assert_eq!(info.bot_scales.get("Boss"), Some(&1.6));

        let state = LobbyIn::parse(
            r#"{"type":"campaign:state","length":8,"unlockedThrough":2,
                "missions":[{"id":"cartel-contract","number":1,"map":"washoi_amphi2","mapTitle":"Amphi 2","gametype":"amphi",
                  "bots":[{"name":"King","difficulty":"hard","side":"enemy"}],"models":{"ally":"robot","enemy":"cartel"}}],
                "clears":[{"missionId":"cartel-contract","clearedAt":1,"bestTimeMs":2,"bestFrags":12}]}"#,
        );
        let Some(LobbyIn::CampaignState(state)) = state else {
            panic!("{state:?}");
        };
        assert_eq!(
            (state.length, state.unlocked_through, state.clears[0].best_frags),
            (8, 2, 12)
        );

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
