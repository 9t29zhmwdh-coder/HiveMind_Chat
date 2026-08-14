//! Provider for endpoints speaking the OpenAI chat completions dialect.
//!
//! One implementation covers OpenAI, LM Studio, vLLM, llama.cpp, Groq and
//! Together: they differ only in base URL and credential, both of which come
//! from the provider entry.

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::sse::{line_stream, sse_payload};
use super::{build_client, error_from_response, ChatChunk, ChatRequest, ChatStream, ModelProvider};
use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{HiveError, Result};
use crate::model::{Role, TokenUsage};
use crate::secrets::SecretRef;

pub struct OpenAiProvider {
    id: String,
    base_url: String,
    secret: Option<SecretRef>,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            id: config.id.clone(),
            base_url: config.resolved_base_url(),
            secret: config.secret_ref()?,
            client: build_client(config)?,
        })
    }

    /// Local servers such as LM Studio accept any bearer token, so the
    /// credential stays optional even though the dialect defines one.
    fn authorised(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        match &self.secret {
            Some(secret) => Ok(builder.bearer_auth(secret.resolve()?)),
            None => Ok(builder),
        }
    }
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    stream: bool,
    temperature: f32,
    max_tokens: u32,
    stream_options: StreamOptions,
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<ChunkUsage>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<ChoiceDelta>,
}

#[derive(Deserialize)]
struct ChoiceDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct ChunkUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
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

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAi
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let request = self.authorised(self.client.get(format!("{}/models", self.base_url)))?;
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(error_from_response(&self.id, response).await);
        }
        let models: ModelsResponse = response.json().await?;
        Ok(models.data.into_iter().map(|m| m.id).collect())
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let mut messages = Vec::with_capacity(request.turns.len() + 1);
        if !request.system.is_empty() {
            messages.push(OpenAiMessage {
                role: "system",
                content: &request.system,
            });
        }
        for turn in &request.turns {
            messages.push(OpenAiMessage {
                role: role_name(turn.role),
                content: &turn.content,
            });
        }

        let body = OpenAiRequest {
            model: &request.model,
            messages,
            stream: true,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream_options: StreamOptions {
                include_usage: true,
            },
        };

        let http = self.authorised(
            self.client
                .post(format!("{}/chat/completions", self.base_url))
                .json(&body),
        )?;
        let response = http.send().await?;
        if !response.status().is_success() {
            return Err(error_from_response(&self.id, response).await);
        }

        let provider_id = self.id.clone();
        let mut usage = TokenUsage::default();
        let mut finished = false;
        let stream = line_stream(response.bytes_stream()).filter_map(move |line| {
            let mapped = match line {
                Ok(line) => parse_line(&provider_id, &line, &mut usage, &mut finished),
                Err(err) => Some(Err(err)),
            };
            async move { mapped }
        });
        Ok(Box::pin(stream))
    }
}

/// Maps one SSE line to a chunk.
///
/// The dialect has no explicit completion event: the stream ends with
/// `data: [DONE]`, which the SSE layer filters out. `finished` therefore turns
/// the end of the line stream into a single `Done` chunk carrying the usage
/// that the final chunk reported.
fn parse_line(
    provider_id: &str,
    line: &str,
    usage: &mut TokenUsage,
    finished: &mut bool,
) -> Option<Result<ChatChunk>> {
    if line.trim() == "data: [DONE]" {
        if *finished {
            return None;
        }
        *finished = true;
        return Some(Ok(ChatChunk::Done(*usage)));
    }

    let payload = sse_payload(line)?;
    let chunk: StreamChunk = match serde_json::from_str(payload) {
        Ok(chunk) => chunk,
        Err(err) => return Some(Err(HiveError::provider(provider_id, err))),
    };

    if let Some(counts) = chunk.usage {
        usage.input_tokens = counts.prompt_tokens;
        usage.output_tokens = counts.completion_tokens;
    }

    let text = chunk
        .choices
        .into_iter()
        .filter_map(|choice| choice.delta.and_then(|d| d.content))
        .collect::<String>();
    if text.is_empty() {
        return None;
    }
    Some(Ok(ChatChunk::Delta(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (TokenUsage, bool) {
        (TokenUsage::default(), false)
    }

    #[test]
    fn content_deltas_are_forwarded() {
        let (mut usage, mut finished) = state();
        let line = r#"data: {"choices":[{"delta":{"content":"Hallo"}}]}"#;
        match parse_line("groq", line, &mut usage, &mut finished) {
            Some(Ok(ChatChunk::Delta(text))) => assert_eq!(text, "Hallo"),
            _ => panic!("expected a delta"),
        }
    }

    #[test]
    fn role_only_deltas_are_skipped() {
        let (mut usage, mut finished) = state();
        let line = r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#;
        assert!(parse_line("groq", line, &mut usage, &mut finished).is_none());
    }

    #[test]
    fn done_marker_emits_usage_once() {
        let (mut usage, mut finished) = state();
        parse_line(
            "groq",
            r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":34}}"#,
            &mut usage,
            &mut finished,
        );
        match parse_line("groq", "data: [DONE]", &mut usage, &mut finished) {
            Some(Ok(ChatChunk::Done(total))) => {
                assert_eq!(total.input_tokens, 12);
                assert_eq!(total.output_tokens, 34);
            }
            _ => panic!("expected a done chunk"),
        }
        assert!(parse_line("groq", "data: [DONE]", &mut usage, &mut finished).is_none());
    }

    #[test]
    fn malformed_payloads_surface_as_provider_errors() {
        let (mut usage, mut finished) = state();
        assert!(matches!(
            parse_line("groq", "data: {not json}", &mut usage, &mut finished),
            Some(Err(HiveError::Provider { .. }))
        ));
    }
}
