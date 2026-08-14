//! A provider that answers from a script instead of from a model.
//!
//! It lets the integration tests drive the orchestrator end to end without a
//! network: the policies, the prompt projection and the transcript are exercised
//! for real, only the model is replaced.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;
use hive_core::{
    ChatChunk, ChatRequest, ChatTurn, ModelProvider, ProviderKind, Result, TokenUsage,
};

/// What one call to the scripted provider produces.
#[derive(Clone)]
pub enum Reply {
    /// Streams this text back, split into several deltas.
    Text(String),
    /// Fails the turn, so the caller can assert on recovery behaviour.
    Failure(String),
    /// Streams nothing, which the orchestrator treats as a failed turn.
    Empty,
}

/// One recorded request, so a test can assert on what the model actually saw.
#[derive(Clone, Debug)]
pub struct Seen {
    pub model: String,
    pub system: String,
    pub turns: Vec<ChatTurn>,
}

impl Seen {
    /// The concatenated user-visible content of the request.
    pub fn transcript(&self) -> String {
        self.turns
            .iter()
            .map(|turn| turn.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct ScriptedProvider {
    id: String,
    /// Replies handed out in order; the last one repeats once exhausted.
    replies: Mutex<Vec<Reply>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl ScriptedProvider {
    pub fn new(id: &str, replies: Vec<Reply>) -> Arc<Self> {
        Arc::new(Self {
            id: id.to_string(),
            replies: Mutex::new(replies),
            seen: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Answers every call with the same text.
    pub fn always(id: &str, text: &str) -> Arc<Self> {
        Self::new(id, vec![Reply::Text(text.to_string())])
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .expect("the recorder lock is poisoned")
            .clone()
    }

    pub fn call_count(&self) -> usize {
        self.requests().len()
    }

    fn next_reply(&self) -> Reply {
        let mut replies = self.replies.lock().expect("the script lock is poisoned");
        match replies.len() {
            0 => Reply::Empty,
            1 => replies[0].clone(),
            _ => replies.remove(0),
        }
    }
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["scripted".to_string()])
    }

    async fn chat(&self, request: ChatRequest) -> Result<hive_core::provider::ChatStream> {
        self.seen
            .lock()
            .expect("the recorder lock is poisoned")
            .push(Seen {
                model: request.model.clone(),
                system: request.system.clone(),
                turns: request.turns.clone(),
            });

        match self.next_reply() {
            Reply::Failure(message) => Err(hive_core::HiveError::provider(&self.id, message)),
            Reply::Empty => Ok(Box::pin(stream::empty())),
            // Split into words so the test also covers delta accumulation.
            Reply::Text(text) => {
                let mut chunks: Vec<Result<ChatChunk>> = text
                    .split_inclusive(' ')
                    .map(|part| Ok(ChatChunk::Delta(part.to_string())))
                    .collect();
                chunks.push(Ok(ChatChunk::Done(TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                })));
                Ok(Box::pin(stream::iter(chunks)))
            }
        }
    }
}
