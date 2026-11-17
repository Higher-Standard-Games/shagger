# Networking and lobby

> Behaviour: SHAG's [Networking](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/networking.md) and [Lobby server](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/lobby-server.md). The bytes on the wire, the handshake, the acknowledgements and the lobby's JSON messages are the same, so a native client and a browser can play in one match. RUST.md 4 and 18 describe the protocol field by field.

Source: `crates/shagger_shared/src/net/`, `crates/shagger/src/network/`, `crates/shagger/src/lobby/`.

## The codec (`shagger_shared::net`)

The codec is byte for byte the TypeScript's: little-endian, strings as a u16 byte length then UTF-8.

- `binary_protocol::{BinaryEncoder, BinaryDecoder}` port `BinaryProtocol.ts`'s primitives. The TypeScript takes JavaScript numbers and lets `DataView` convert them; here every writer takes the wire type, and a caller holding an `f64` that may be fractional, negative or out of range converts with `js::to_uint8` and its siblings to get the same bytes ([JavaScript parity](./js-parity.md)).
- `command` encodes and decodes input commands.
- `message_codec` ports `MessageCodec.ts`'s envelopes. Every message starts with its `MessageType` byte, and decoders check it first, as the TypeScript does.
- `snapshot` decodes snapshots, entities, players and pickup state into `game_state::GameState<E>`. The TypeScript stores untyped components by name; here each one the client can see is a typed, optional field.

Every decoder is checked against bytes and decoded states produced by SHAG's own `BinaryProtocol` ([Golden vectors](../tools/golden-vectors.md)).

### Snapshot events

SHAG's `BinaryProtocol.ts` hardcodes Washoi's event codes. In SHAGGER the event catalogue belongs to the game (RUST.md 14.2): the snapshot decoder reads the event section through a trait the game implements.

```rust
use shagger::shared::net::{DecodeError, binary_protocol::BinaryDecoder};
use shagger::shared::net::snapshot::EventCodec;

struct MyEvents;

impl EventCodec for MyEvents {
    type Event = MyEvent;
    fn decode_event(&self, code: u8, d: &mut BinaryDecoder<'_>) -> Result<Option<MyEvent>, DecodeError> {
        // Ok(None) for an unknown code: nothing is read and nothing is pushed,
        // as in the TypeScript switch.
        Ok(None)
    }
}
```

The bytes on the wire do not change. `snapshot::NoEvents` is a codec that knows no events, for tests.

## The game server connection

SHAG's `NetworkGameHost.ts` relies on the browser's event loop and WebSocket. SHAGGER splits it in two:

- `network::client::hosts::network_game_host::NetworkGameHost` is the connection logic without the I/O: the handshake (Join, Welcome, JoinAccept, ResumeToken), snapshot acceptance and acknowledgements, clock sync, the stall watchdog, commands and the reliable requests. Its owner feeds it what the socket delivers and the time, and sends the messages it returns. What the browser host also does (music, interpolation for rendering) belongs to the game and the renderer.
- `network::client::network_session::NetworkSession` is that owner for a WebSocket, on a network thread of its own with a single-threaded tokio runtime. It opens `ws://` or `wss://` (over rustls, see [Native runtime](./native-runtime.md#tls)), runs the host's timers, and on a drop takes the seat back with the resume token, retrying until the server's grace has passed.

```rust
use shagger::network::client::network_session::{NetworkSession, SessionConfig, SessionEvent, SessionRequest};

let session = NetworkSession::start(
    SessionConfig { url, player_name, admission_ticket },
    MyEvents,
);

// Once a frame; neither call blocks.
for event in session.poll_events() {
    match event {
        SessionEvent::Host(host_event) => { /* ServerInfo, Joined, Snapshot, ... */ }
        SessionEvent::Resuming { attempt } => { /* show "reconnecting" */ }
        SessionEvent::Closed(reason) => { /* the session is over */ }
    }
}
session.send(SessionRequest::Ready);
```

`HostEvent::ServerInfo` arrives with the server's Welcome, when the map should load. `HostEvent::Snapshot` carries a decoded, acknowledged `GameState`. `HostEvent::Stall` reports snapshots that stopped while the socket stayed open.

### Pieces the game uses directly

| Module | SHAG | Does |
| --- | --- | --- |
| `snapshot_receiver` | `SnapshotReceiver.ts` | drops snapshots that are not newer than the latest, and deltas whose baseline is gone |
| `snapshot_buffer` | `SnapshotBuffer.ts` | the time-based interpolation buffer remote entities are drawn from; the caller passes the clock and the `cl_interp`/`cl_timenudge` values |
| `clock_sync` | `ClockSync.ts` | the NTP-like offset: the best 75% of up to 16 samples by round trip |
| `connection_watchdog` | `ConnectionWatchdog.ts` | notices when snapshots stop; `skip_gap` excuses a gap the client caused |
| `http_fetch` | none | HTTP(S) downloads with the browser client's idle and total deadlines, on a thread of its own |

Prediction and reconciliation (`V2NetworkClient.ts`) and the simulation they run are Washoi's and live in wash-fps-rust. SHAGGER provides the contracts they are written against: `network::systems::movement::movement_profile`, `network::components::input_component`'s button bits, and `shagger_shared::server::simulation::input_sanitizer`, the usercmd clamp the server and the predicting client share.

## The lobby

`lobby::lobby_client::LobbyClient` ports `LobbyClient.ts`. Like the game connection it runs its WebSocket on a thread of its own: the request methods queue a JSON message, and `poll_events` returns what arrived.

```rust
use shagger::lobby::lobby_client::{LobbyClient, LobbyEvent};

let lobby = LobbyClient::connect(&lobby_url);
lobby.auth(Some(&player_key));
for event in lobby.poll_events() {
    if let LobbyEvent::Message(message) = event { /* ... */ }
}
```

Messages are sent as `JSON.stringify` would write them, with absent optional fields left out. `lobby::types` types the server messages the client acts on; every other message arrives as `LobbyIn::Other` with its JSON, so a lobby that adds a message never breaks the client. Field names follow the TypeScript (camelCase).

`player_identity` ports `PlayerIdentity.ts`'s rules for player keys (120 random bits as 24 Crockford base32 characters) and names, so the client reads a pasted key and judges a name as the lobby does.

### Settings sync

`lobby::settings_sync::SettingsSync` ports `SettingsSync.ts`. On login the account's copy applies unless this machine changed its own since the last sync and the account's is older; an account with none takes this machine's. While logged in, a change uploads two seconds after it settles, and `flush` uploads what is left on the way out.

The browser keeps its side in local storage. Here the sync state, the backup and the synced storage entries (bindings, loadouts) are files in the config folder. Which settings sync is `shagger_shared::settings::synced_settings`: key and command bindings, game preferences (the `cl_`, `cg_`, `s_` and `m_` cvars and colour correction), the HUD layout, the theme and the remembered loadouts. Graphics and anything tied to the device stay. The blob is compared as text, so `serialize_synced_settings` writes it exactly as `JSON.stringify` does, with its keys sorted.
