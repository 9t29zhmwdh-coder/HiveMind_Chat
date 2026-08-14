use std::collections::HashSet;
use std::sync::Arc;

use hive_core::{HiveConfig, Orchestrator, ProviderRegistry, Result, Store};
use tokio::sync::{Mutex, Semaphore};

/// How many rooms may stream at the same time.
///
/// Each active room fans out to every agent in it, so this bounds the total
/// number of upstream connections a small homelab box has to sustain.
const MAX_CONCURRENT_TURNS: usize = 4;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub config: Arc<HiveConfig>,
    pub registry: ProviderRegistry,
    pub orchestrator: Arc<Orchestrator>,
    pub access_token: Option<Arc<String>>,
    turn_slots: Arc<Semaphore>,
    busy_rooms: Arc<Mutex<HashSet<String>>>,
}

impl AppState {
    pub fn new(store: Store, config: HiveConfig, access_token: Option<String>) -> Result<Self> {
        let registry = ProviderRegistry::from_config(&config)?;
        Ok(Self {
            store,
            config: Arc::new(config),
            orchestrator: Arc::new(Orchestrator::new(registry.clone())),
            registry,
            access_token: access_token.map(Arc::new),
            turn_slots: Arc::new(Semaphore::new(MAX_CONCURRENT_TURNS)),
            busy_rooms: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Claims the right to run a turn in `room_id`.
    ///
    /// Two turns in one room would interleave their messages into the same
    /// transcript, so a room admits one at a time; the returned guard releases
    /// the claim on drop, including when the client disconnects mid-turn.
    pub async fn claim_room(&self, room_id: &str) -> Option<TurnGuard> {
        let permit = Arc::clone(&self.turn_slots).try_acquire_owned().ok()?;
        let mut busy = self.busy_rooms.lock().await;
        if !busy.insert(room_id.to_string()) {
            return None;
        }
        Some(TurnGuard {
            room_id: room_id.to_string(),
            busy_rooms: Arc::clone(&self.busy_rooms),
            _permit: permit,
        })
    }

    pub fn token_matches(&self, presented: Option<&str>) -> bool {
        match (&self.access_token, presented) {
            (None, _) => true,
            (Some(expected), Some(given)) => {
                constant_time_eq(expected.as_bytes(), given.as_bytes())
            }
            (Some(_), None) => false,
        }
    }
}

/// Compares two secrets without an early exit, so a wrong token cannot be
/// discovered one byte at a time by timing the response.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub struct TurnGuard {
    room_id: String,
    busy_rooms: Arc<Mutex<HashSet<String>>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        let busy_rooms = Arc::clone(&self.busy_rooms);
        let room_id = std::mem::take(&mut self.room_id);
        tokio::spawn(async move {
            busy_rooms.lock().await.remove(&room_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(token: Option<&str>) -> AppState {
        AppState::new(
            Store::in_memory().unwrap(),
            HiveConfig::local_default(),
            token.map(str::to_string),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_room_admits_only_one_turn_at_a_time() {
        let state = state(None);
        let first = state.claim_room("room-a").await;
        assert!(first.is_some());
        assert!(state.claim_room("room-a").await.is_none());
        assert!(state.claim_room("room-b").await.is_some());
    }

    #[tokio::test]
    async fn releasing_a_claim_lets_the_next_turn_in() {
        let state = state(None);
        drop(state.claim_room("room-a").await);
        // The guard clears its entry on a spawned task; yield until it lands.
        for _ in 0..50 {
            if state.claim_room("room-a").await.is_some() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the room stayed claimed after the guard was dropped");
    }

    #[tokio::test]
    async fn concurrent_turns_are_capped() {
        let state = state(None);
        let mut guards = Vec::new();
        for index in 0..MAX_CONCURRENT_TURNS {
            guards.push(state.claim_room(&format!("room-{index}")).await);
        }
        assert!(guards.iter().all(Option::is_some));
        assert!(state.claim_room("one-too-many").await.is_none());
    }

    #[test]
    fn token_check_is_open_when_no_token_is_configured() {
        assert!(state(None).token_matches(None));
    }

    #[test]
    fn token_check_rejects_wrong_and_missing_tokens() {
        let state = state(Some("s3cret"));
        assert!(state.token_matches(Some("s3cret")));
        assert!(!state.token_matches(Some("s3cre")));
        assert!(!state.token_matches(Some("wrong!")));
        assert!(!state.token_matches(None));
    }
}
