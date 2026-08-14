//! Anthropic Messages API provider.

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::sse::{line_stream, sse_payload};
use super::{build_client, error_from_response, ChatChunk, ChatRequest, ChatStream, ModelProvider};
use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{HiveError, Result};
use crate::model::{Role, TokenUsage};
use crate::secrets::SecretRef;

const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    id: String,
    base_url: String,
    secret: SecretRef,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        config.validate()?;
        let secret = config.secret_ref()?.ok_or_else(|| {
            HiveError::Config(format!("provider '{}' has no api_key_env", config.id))
        })?;
        Ok(Self {
            id: config.id.clone(),
            base_url: config.resolved_base_url(),
            secret,
            client: build_client(config)?,
        })
    }

    /// Builds a request with the credential attached.
    ///
    /// The key is read per request and never stored on the struct, so a memory
    /// dump of a long-running server does not hold it indefinitely.
    fn authorised(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        let key = self.secret.resolve()?;
        Ok(builder
            .header("x-api-key", key)
            .header("anthropic-version", API_VERSION))
    }
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// The Messages API knows only `user` and `assistant`; the system prompt is a
/// top-level field, so a system turn inside the history would be rejected.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::System | Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let request = self.authorised(self.client.get(format!("{}/v1/models", self.base_url)))?;
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(error_from_response(&self.id, response).await);
        }
        let models: ModelsResponse = response.json().await?;
        Ok(models.data.into_iter().map(|m| m.id).collect())
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let messages: Vec<AnthropicMessage> = request
            .turns
            .iter()
            .map(|turn| AnthropicMessage {
                role: role_name(turn.role),
                content: &turn.content,
            })
            .collect();

        let body = build_body(&request, &messages);
        let http = self.authorised(
            self.client
                .post(format!("{}/v1/messages", self.base_url))
                .json(&body),
        )?;
        let response = http.send().await?;
        if !response.status().is_success() {
            return Err(error_from_response(&self.id, response).await);
        }

        let provider_id = self.id.clone();
        let mut usage = TokenUsage::default();
        let stream = line_stream(response.bytes_stream()).filter_map(move |line| {
            let mapped = match line {
                Ok(line) => parse_event(&provider_id, &line, &mut usage),
                Err(err) => Some(Err(err)),
            };
            async move { mapped }
        });
        Ok(Box::pin(stream))
    }
}

/// Builds the request body.
///
/// `temperature` is deliberately omitted: current Anthropic models reject the
/// sampling parameters with a 400, so the agent's temperature is honoured only
/// by providers that still accept it. Thinking is switched off unless the agent
/// asks for it, because reasoning tokens come out of the same `max_tokens`
/// budget as the answer.
fn build_body(request: &ChatRequest, messages: &[AnthropicMessage]) -> Value {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens,
        "stream": true,
        "messages": messages,
    });
    if !request.system.is_empty() {
        body["system"] = json!(request.system);
    }
    if !request.reasoning {
        body["thinking"] = json!({ "type": "disabled" });
    }
    body
}

#[derive(Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    delta: Option<EventDelta>,
    #[serde(default)]
    message: Option<EventMessage>,
    #[serde(default)]
    usage: Option<EventUsage>,
    #[serde(default)]
    error: Option<EventError>,
}

#[derive(Deserialize)]
struct EventDelta {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct EventMessage {
    #[serde(default)]
    usage: Option<EventUsage>,
}

#[derive(Deserialize, Default)]
struct EventUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Deserialize)]
struct EventError {
    #[serde(default)]
    message: String,
}

/// Maps one SSE line to a chunk.
///
/// `usage` accumulates across events because the input count arrives with
/// `message_start` while the output count only appears in `message_delta`.
fn parse_event(provider_id: &str, line: &str, usage: &mut TokenUsage) -> Option<Result<ChatChunk>> {
    let payload = sse_payload(line)?;
    let event: StreamEvent = match serde_json::from_str(payload) {
        Ok(event) => event,
        Err(err) => return Some(Err(HiveError::provider(provider_id, err))),
    };

    match event.kind.as_str() {
        "message_start" => {
            if let Some(counts) = event.message.and_then(|m| m.usage) {
                usage.input_tokens = counts.input_tokens;
                usage.output_tokens = counts.output_tokens;
            }
            None
        }
        "content_block_delta" => match event.delta {
            // `thinking_delta` events are skipped: reasoning is not part of the
            // transcript other agents see.
            Some(delta) if delta.kind == "text_delta" && !delta.text.is_empty() => {
                Some(Ok(ChatChunk::Delta(delta.text)))
            }
            _ => None,
        },
        "message_delta" => {
            if let Some(counts) = event.usage {
                usage.output_tokens = counts.output_tokens;
            }
            None
        }
        "message_stop" => Some(Ok(ChatChunk::Done(*usage))),
        "error" => {
            let message = event.error.map(|e| e.message).unwrap_or_default();
            Some(Err(HiveError::provider(provider_id, message)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ChatTurn;

    fn request(reasoning: bool) -> ChatRequest {
        ChatRequest {
            model: "claude-opus-5".into(),
            system: "You are Scout.".into(),
            turns: vec![ChatTurn::user("Hi")],
            temperature: 0.9,
            max_tokens: 512,
            reasoning,
        }
    }

    #[test]
    fn body_omits_sampling_parameters() {
        let body = build_body(&request(false), &[]);
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["system"], "You are Scout.");
    }

    #[test]
    fn reasoning_agents_leave_thinking_at_the_default() {
        let body = build_body(&request(true), &[]);
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn text_deltas_are_forwarded_and_thinking_deltas_are_not() {
        let mut usage = TokenUsage::default();
        let text =
            r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}"#;
        let thinking = r#"data: {"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"..."}}"#;

        match parse_event("anthropic", text, &mut usage) {
            Some(Ok(ChatChunk::Delta(t))) => assert_eq!(t, "Hi"),
            _ => panic!("expected a text delta"),
        }
        assert!(parse_event("anthropic", thinking, &mut usage).is_none());
    }

    #[test]
    fn usage_is_collected_across_events() {
        let mut usage = TokenUsage::default();
        parse_event(
            "anthropic",
            r#"data: {"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":0}}}"#,
            &mut usage,
        );
        parse_event(
            "anthropic",
            r#"data: {"type":"message_delta","usage":{"output_tokens":40}}"#,
            &mut usage,
        );
        match parse_event("anthropic", r#"data: {"type":"message_stop"}"#, &mut usage) {
            Some(Ok(ChatChunk::Done(total))) => {
                assert_eq!(total.input_tokens, 25);
                assert_eq!(total.output_tokens, 40);
            }
            _ => panic!("expected a done chunk"),
        }
    }

    #[test]
    fn error_events_become_provider_errors() {
        let mut usage = TokenUsage::default();
        let line =
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert!(matches!(
            parse_event("anthropic", line, &mut usage),
            Some(Err(HiveError::Provider { .. }))
        ));
    }

    #[test]
    fn system_turns_are_mapped_to_user_role() {
        assert_eq!(role_name(Role::System), "user");
        assert_eq!(role_name(Role::Assistant), "assistant");
    }
}
