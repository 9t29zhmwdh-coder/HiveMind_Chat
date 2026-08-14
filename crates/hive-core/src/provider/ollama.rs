//! Ollama provider: the local-first path, no credential involved.

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::sse::line_stream;
use super::{build_client, error_from_response, ChatChunk, ChatRequest, ChatStream, ModelProvider};
use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{HiveError, Result};
use crate::model::{Role, TokenUsage};

pub struct OllamaProvider {
    id: String,
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            id: config.id.clone(),
            base_url: config.resolved_base_url(),
            client: build_client(config)?,
        })
    }
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Deserialize)]
struct OllamaChunk {
    #[serde(default)]
    message: Option<OllamaChunkMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
    /// Present when the daemon rejects the request mid-stream.
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct OllamaChunkMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagEntry>,
}

#[derive(Deserialize)]
struct TagEntry {
    name: String,
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(error_from_response(&self.id, response).await);
        }
        let tags: TagsResponse = response.json().await?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let mut messages = Vec::with_capacity(request.turns.len() + 1);
        if !request.system.is_empty() {
            messages.push(OllamaMessage {
                role: "system",
                content: &request.system,
            });
        }
        for turn in &request.turns {
            messages.push(OllamaMessage {
                role: role_name(turn.role),
                content: &turn.content,
            });
        }

        let body = OllamaRequest {
            model: &request.model,
            messages,
            stream: true,
            options: OllamaOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            },
        };

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(error_from_response(&self.id, response).await);
        }

        let provider_id = self.id.clone();
        let stream = line_stream(response.bytes_stream()).filter_map(move |line| {
            let provider_id = provider_id.clone();
            async move {
                match line {
                    Ok(line) => parse_chunk(&provider_id, &line),
                    Err(err) => Some(Err(err)),
                }
            }
        });
        Ok(Box::pin(stream))
    }
}

/// Ollama emits one JSON object per line; blank lines are keep-alives.
fn parse_chunk(provider_id: &str, line: &str) -> Option<Result<ChatChunk>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let chunk: OllamaChunk = match serde_json::from_str(trimmed) {
        Ok(chunk) => chunk,
        Err(err) => return Some(Err(HiveError::provider(provider_id, err))),
    };
    if let Some(message) = chunk.error {
        return Some(Err(HiveError::provider(provider_id, message)));
    }
    if chunk.done {
        return Some(Ok(ChatChunk::Done(TokenUsage {
            input_tokens: chunk.prompt_eval_count,
            output_tokens: chunk.eval_count,
        })));
    }
    let content = chunk.message.map(|m| m.content).unwrap_or_default();
    if content.is_empty() {
        return None;
    }
    Some(Ok(ChatChunk::Delta(content)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_lines_yield_text() {
        let line = r#"{"message":{"role":"assistant","content":"Hallo"},"done":false}"#;
        match parse_chunk("local", line) {
            Some(Ok(ChatChunk::Delta(text))) => assert_eq!(text, "Hallo"),
            other => panic!("unexpected chunk: {other:?}", other = other.is_some()),
        }
    }

    #[test]
    fn final_line_carries_token_usage() {
        let line = r#"{"done":true,"prompt_eval_count":11,"eval_count":42}"#;
        match parse_chunk("local", line) {
            Some(Ok(ChatChunk::Done(usage))) => {
                assert_eq!(usage.input_tokens, 11);
                assert_eq!(usage.output_tokens, 42);
            }
            _ => panic!("expected a done chunk"),
        }
    }

    #[test]
    fn error_lines_become_provider_errors() {
        let line = r#"{"error":"model not found"}"#;
        assert!(matches!(
            parse_chunk("local", line),
            Some(Err(HiveError::Provider { .. }))
        ));
    }

    #[test]
    fn blank_and_empty_deltas_are_skipped() {
        assert!(parse_chunk("local", "   ").is_none());
        assert!(parse_chunk("local", r#"{"message":{"content":""},"done":false}"#).is_none());
    }
}
