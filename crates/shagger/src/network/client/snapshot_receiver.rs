//! SHAG `src/engine/network/client/SnapshotReceiver.ts`: live snapshot
//! acceptance shared by the client, bots and headless clients.
//!
//! Snapshots that are not newer than the latest are dropped. A delta whose
//! baseline is no longer held is dropped too (not acknowledged, not
//! published): the server sends full snapshots now and then, so an evicted
//! baseline cannot cause a permanent decode loop. History belongs to a
//! connection, not a player life; reset it on reconnect only.

use std::collections::VecDeque;

use shagger_shared::net::DecodeError;
use shagger_shared::net::game_state::GameState;
use shagger_shared::net::message_codec::{decode_snapshot, read_snapshot_header};
use shagger_shared::net::snapshot::EventCodec;

/// Baselines kept for delta decoding.
pub const HISTORY_SIZE: usize = 128;

pub struct SnapshotReceiver<E> {
    /// Oldest first. Each keeps only what delta decoding reads.
    history: VecDeque<GameState<E>>,
    latest_tick: Option<u32>,
    /// Snapshots dropped for a missing baseline, for diagnostics.
    pub dropped_missing_baseline: u64,
}

impl<E> Default for SnapshotReceiver<E> {
    fn default() -> Self {
        Self {
            history: VecDeque::new(),
            latest_tick: None,
            dropped_missing_baseline: 0,
        }
    }
}

impl<E> SnapshotReceiver<E> {
    /// `receive`: the decoded snapshot, or `None` when it is stale or its
    /// baseline is gone (its one-tick events are lost with it).
    ///
    /// # Errors
    /// A malformed snapshot, which the TypeScript throws too.
    pub fn receive<C: EventCodec<Event = E>>(
        &mut self,
        data: &[u8],
        codec: &C,
    ) -> Result<Option<GameState<E>>, DecodeError> {
        let tick = read_snapshot_header(data)?.tick;
        if self.latest_tick.is_some_and(|latest| tick <= latest) {
            return Ok(None);
        }
        let history = &self.history;
        let snapshot = match decode_snapshot(data, |t| history.iter().find(|s| s.tick == t), codec) {
            Ok(s) => s,
            Err(DecodeError::MissingBaseline { .. }) => {
                self.dropped_missing_baseline += 1;
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        self.history.push_back(GameState {
            tick: snapshot.tick,
            entities: snapshot.entities.clone(),
            players: snapshot.players.clone(),
            ..GameState::default()
        });
        while self.history.len() > HISTORY_SIZE {
            self.history.pop_front();
        }
        self.latest_tick = Some(snapshot.tick);
        Ok(Some(snapshot))
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.latest_tick = None;
    }
}
