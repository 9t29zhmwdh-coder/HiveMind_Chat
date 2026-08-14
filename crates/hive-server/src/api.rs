//! REST endpoints for configuration, rooms and transcripts.

use axum::extract::{Path, Query, State};
use axum::Json;
use hive_core::{Agent, Message, Room, RoomSummary, TurnPolicy, VERSION};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// How many messages a room hands back by default.
const DEFAULT_TRANSCRIPT_LIMIT: u32 = 500;

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
}

pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: VERSION,
    })
}

/// A provider as the web UI sees it: never the key, only whether it resolves.
#[derive(Serialize)]
pub struct ProviderView {
    pub id: String,
    pub label: String,
    pub kind: hive_core::ProviderKind,
    pub base_url: String,
    pub local: bool,
    pub credential_env: Option<String>,
    pub credential_available: bool,
}

pub async fn list_providers(State(state): State<AppState>) -> Json<Vec<ProviderView>> {
    let views = state
        .config
        .providers
        .iter()
        .map(|provider| ProviderView {
            id: provider.id.clone(),
            label: display_label(provider),
            kind: provider.kind,
            base_url: provider.resolved_base_url(),
            local: provider.is_local(),
            credential_env: provider.api_key_env.clone(),
            credential_available: provider
                .secret_ref()
                .ok()
                .flatten()
                .is_none_or(|secret| secret.is_available()),
        })
        .collect();
    Json(views)
}

fn display_label(provider: &hive_core::ProviderConfig) -> String {
    match provider.label.trim().is_empty() {
        true => provider.id.clone(),
        false => provider.label.clone(),
    }
}

pub async fn provider_models(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> ApiResult<Json<Vec<String>>> {
    let provider = state.registry.get(&provider_id)?;
    let mut models = provider.list_models().await?;
    models.sort();
    Ok(Json(models))
}

pub async fn list_policies() -> Json<Vec<PolicyView>> {
    Json(
        TurnPolicy::ALL
            .iter()
            .map(|policy| PolicyView::of(*policy))
            .collect(),
    )
}

#[derive(Serialize)]
pub struct PolicyView {
    pub id: &'static str,
    pub summary: &'static str,
}

impl PolicyView {
    fn of(policy: TurnPolicy) -> Self {
        let summary = match policy {
            TurnPolicy::Parallel => {
                "Every agent answers the same prompt independently, for side-by-side comparison."
            }
            TurnPolicy::RoundRobin => "Agents speak in turn and each sees everything said before.",
            TurnPolicy::Debate => {
                "Agents receive alternating stances and challenge the previous speaker."
            }
            TurnPolicy::Moderated => {
                "A moderator agent picks who speaks next, one speaker per round."
            }
            TurnPolicy::Consensus => "Agents discuss, then each casts an explicit final vote.",
        };
        Self {
            id: policy.as_str(),
            summary,
        }
    }
}

#[derive(Deserialize)]
pub struct RoomInput {
    pub name: String,
    #[serde(default)]
    pub topic: String,
    pub policy: TurnPolicy,
    #[serde(default = "one")]
    pub rounds: u32,
    #[serde(default)]
    pub moderator_id: Option<String>,
    #[serde(default)]
    pub agents: Vec<AgentInput>,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
pub struct AgentInput {
    /// Present when updating an existing agent; a new agent gets a fresh id.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub persona: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub colour: Option<String>,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl RoomInput {
    /// Builds a room, keeping `existing`'s id and creation time when updating.
    fn into_room(self, existing: Option<&Room>) -> Room {
        let mut room = match existing {
            Some(room) => room.clone(),
            None => Room::new(&self.name, self.policy),
        };
        room.name = self.name;
        room.topic = self.topic;
        room.policy = self.policy;
        room.rounds = self.rounds;
        room.moderator_id = self.moderator_id;
        room.agents = self
            .agents
            .into_iter()
            .map(AgentInput::into_agent)
            .collect();
        room
    }
}

impl AgentInput {
    fn into_agent(self) -> Agent {
        let mut agent = Agent::new(&self.name, &self.provider_id, &self.model);
        if let Some(id) = self.id {
            agent.id = id;
        }
        agent.persona = self.persona;
        agent.reasoning = self.reasoning;
        agent.enabled = self.enabled;
        if let Some(temperature) = self.temperature {
            agent.temperature = temperature;
        }
        if let Some(max_tokens) = self.max_tokens {
            agent.max_tokens = max_tokens;
        }
        if let Some(colour) = self.colour {
            agent.colour = colour;
        }
        agent
    }
}

/// Rejects agents pointing at providers this instance does not have, which
/// would otherwise only fail once the room is already mid-turn.
fn validate_providers(state: &AppState, room: &Room) -> ApiResult<()> {
    for agent in &room.agents {
        state.registry.get(&agent.provider_id).map_err(|_| {
            ApiError::bad_request(format!(
                "agent '{}' references the unknown provider '{}'",
                agent.name, agent.provider_id
            ))
        })?;
    }
    Ok(())
}

pub async fn list_rooms(State(state): State<AppState>) -> ApiResult<Json<Vec<RoomSummary>>> {
    Ok(Json(state.store.list_rooms().await?))
}

pub async fn create_room(
    State(state): State<AppState>,
    Json(input): Json<RoomInput>,
) -> ApiResult<Json<Room>> {
    let room = input.into_room(None);
    validate_providers(&state, &room)?;
    state.store.save_room(&room).await?;
    Ok(Json(room))
}

#[derive(Serialize)]
pub struct RoomDetail {
    #[serde(flatten)]
    pub room: Room,
    pub messages: Vec<Message>,
}

pub async fn get_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> ApiResult<Json<RoomDetail>> {
    let room = state.store.load_room(&room_id).await?;
    let messages = state
        .store
        .load_messages(&room_id, DEFAULT_TRANSCRIPT_LIMIT)
        .await?;
    Ok(Json(RoomDetail { room, messages }))
}

pub async fn update_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(input): Json<RoomInput>,
) -> ApiResult<Json<Room>> {
    let existing = state.store.load_room(&room_id).await?;
    let room = input.into_room(Some(&existing));
    validate_providers(&state, &room)?;
    state.store.save_room(&room).await?;
    Ok(Json(room))
}

pub async fn delete_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> ApiResult<Json<Deleted>> {
    state.store.delete_room(&room_id).await?;
    Ok(Json(Deleted { deleted: true }))
}

#[derive(Serialize)]
pub struct Deleted {
    pub deleted: bool,
}

pub async fn clear_transcript(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> ApiResult<Json<Deleted>> {
    state.store.load_room(&room_id).await?;
    state.store.clear_messages(&room_id).await?;
    Ok(Json(Deleted { deleted: true }))
}

#[derive(Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Renders the transcript as Markdown for archiving outside the app.
pub async fn export_transcript(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<String> {
    let room = state.store.load_room(&room_id).await?;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_TRANSCRIPT_LIMIT)
        .clamp(1, 5_000);
    let messages = state.store.load_messages(&room_id, limit).await?;
    Ok(render_markdown(&room, &messages))
}

fn render_markdown(room: &Room, messages: &[Message]) -> String {
    let mut out = format!("# {}\n\n", room.name);
    if !room.topic.trim().is_empty() {
        out.push_str(&format!("**Topic:** {}\n\n", room.topic));
    }
    out.push_str(&format!(
        "**Policy:** {} · **Rounds:** {} · **Agents:** {}\n\n---\n\n",
        room.policy.as_str(),
        room.rounds,
        room.agents
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for message in messages {
        out.push_str(&format!(
            "### {} · {}\n\n{}\n\n",
            message.speaker,
            message.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            message.content
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::{HiveConfig, Store};

    fn state() -> AppState {
        AppState::new(
            Store::in_memory().unwrap(),
            HiveConfig::local_default(),
            None,
        )
        .unwrap()
    }

    fn input() -> RoomInput {
        RoomInput {
            name: "Lab".into(),
            topic: "Databases".into(),
            policy: TurnPolicy::Debate,
            rounds: 2,
            moderator_id: None,
            agents: vec![AgentInput {
                id: None,
                name: "Scout".into(),
                provider_id: "local".into(),
                model: "llama3:8b".into(),
                persona: "You favour simplicity.".into(),
                temperature: Some(0.4),
                max_tokens: Some(256),
                colour: Some("#8ab4f8".into()),
                reasoning: false,
                enabled: true,
            }],
        }
    }

    #[test]
    fn room_input_applies_defaults_and_overrides() {
        let room = input().into_room(None);
        assert_eq!(room.policy, TurnPolicy::Debate);
        assert_eq!(room.rounds, 2);
        assert_eq!(room.agents[0].temperature, 0.4);
        assert_eq!(room.agents[0].max_tokens, 256);
        assert_eq!(room.agents[0].colour, "#8ab4f8");
    }

    #[test]
    fn updating_a_room_keeps_its_identity() {
        let existing = input().into_room(None);
        let mut changed = input();
        changed.name = "Lab v2".into();
        let updated = changed.into_room(Some(&existing));
        assert_eq!(updated.id, existing.id);
        assert_eq!(updated.created_at, existing.created_at);
        assert_eq!(updated.name, "Lab v2");
    }

    #[test]
    fn agents_referencing_unknown_providers_are_rejected() {
        let state = state();
        let mut room = input().into_room(None);
        assert!(validate_providers(&state, &room).is_ok());

        room.agents[0].provider_id = "does-not-exist".into();
        let error = validate_providers(&state, &room).unwrap_err();
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(error.error.contains("does-not-exist"));
    }

    #[tokio::test]
    async fn provider_view_reports_credential_availability_without_the_value() {
        let mut config = HiveConfig::local_default();
        config.providers.push(
            hive_core::ProviderConfig::new("anthropic-main", hive_core::ProviderKind::Anthropic)
                .with_key_env("HIVEMIND_TEST_UNSET_KEY"),
        );
        let state = AppState::new(Store::in_memory().unwrap(), config, None).unwrap();

        let Json(views) = list_providers(State(state)).await;
        let cloud = views.iter().find(|v| v.id == "anthropic-main").unwrap();
        assert_eq!(
            cloud.credential_env.as_deref(),
            Some("HIVEMIND_TEST_UNSET_KEY")
        );
        assert!(!cloud.credential_available);
        assert!(!cloud.local);
        assert!(
            views
                .iter()
                .find(|v| v.id == "local")
                .unwrap()
                .credential_available
        );
    }

    #[test]
    fn markdown_export_contains_the_header_and_every_turn() {
        let room = input().into_room(None);
        let messages = vec![
            Message::from_user(&room.id, "Which database?"),
            Message::from_agent(&room.id, &room.agents[0], "SQLite.", 1),
        ];
        let markdown = render_markdown(&room, &messages);
        assert!(markdown.starts_with("# Lab"));
        assert!(markdown.contains("**Topic:** Databases"));
        assert!(markdown.contains("**Policy:** debate"));
        assert!(markdown.contains("### Scout"));
        assert!(markdown.contains("SQLite."));
    }

    #[tokio::test]
    async fn every_policy_is_described_for_the_ui() {
        let Json(policies) = list_policies().await;
        assert_eq!(policies.len(), TurnPolicy::ALL.len());
        assert!(policies.iter().all(|p| !p.summary.is_empty()));
    }
}
