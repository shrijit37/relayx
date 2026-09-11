//! Canonical protocol model.
//!
//! Typed internal representation for LLM API protocols. Every adapter converts
//! between a wire format and these types. The model preserves source semantics
//! and represents provider-specific features via explicit extension fields.
//!
//! # Design principles
//!
//! - **Lossless by default**: all information that can be represented is preserved.
//! - **Typed, not `serde_json::Value`**: the core model is strongly typed.
//! - **Extensions**: provider-specific data lives in a typed `extensions` map.
//! - **Deferred tools**: tool references can remain references throughout translation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ─── Protocol identification ────────────────────────────────────────────────

/// Identifies a wire protocol family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OpenAI Chat Completions API (`/v1/chat/completions`).
    OpenAiChatCompletions,
    /// Anthropic Messages API (`/v1/messages`).
    AnthropicMessages,
    /// OpenAI Responses API (`/v1/responses`).
    OpenAiResponses,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::OpenAiChatCompletions => write!(f, "openai_chat_completions"),
            Protocol::AnthropicMessages => write!(f, "anthropic_messages"),
            Protocol::OpenAiResponses => write!(f, "openai_responses"),
        }
    }
}

// ─── Role ────────────────────────────────────────────────────────────────────

/// Message role in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// ─── Content blocks ──────────────────────────────────────────────────────────

/// A single content block within a message. This is the fundamental unit of
/// content representation in the canonical model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content.
    Text(TextContent),
    /// Image content (URL or inline base64).
    Image(ImageContent),
    /// Audio content (URL or inline base64).
    Audio(AudioContent),
    /// Model's internal reasoning/thinking content.
    ///
    /// Anthropic returns these as `thinking` blocks; the model never exposes
    /// them to end-users directly but they must be preserved during translation
    /// (especially the `signature` field, which is required for multi-turn
    /// tool-use continuation).
    Reasoning(ReasoningContent),
    /// Model is requesting to call a tool.
    ToolUse(ToolUseBlock),
    /// Client is providing the result of a tool call.
    ToolResult(ToolResultBlock),
    /// A deferred tool reference — not fully materialized.
    ToolReference(ToolReference),
}

impl ContentBlock {
    /// Return the text content if this is a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text(t) => Some(&t.text),
            _ => None,
        }
    }
}

/// Plain text content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    /// The text content.
    pub text: String,
}

/// An inline image (URL or base64-encoded data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    /// The image source.
    pub source: ImageSource,
}

/// An audio clip (URL or inline base64-encoded data).
///
/// OpenAI wire format (`input_audio`):
/// ```json
/// {"type": "input_audio", "input_audio": {"data": "base64...", "format": "wav"}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioContent {
    /// The audio source.
    pub source: AudioSource,
}

/// Source of audio — either a URL or inline base64 data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioSource {
    /// Audio accessible via URL.
    Url {
        /// The audio URL.
        url: String,
    },
    /// Inline base64-encoded audio.
    Base64 {
        /// MIME type of the audio (e.g., "audio/wav").
        media_type: String,
        /// Base64-encoded audio data.
        data: String,
        /// Provider-specific format hint (e.g., "wav", "pcm16", "mp3").
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
}

/// Model's internal reasoning or thinking content.
///
/// Anthropic returns these as `thinking` blocks when extended thinking is
/// enabled. The `signature` field is **opaque and mandatory** — it must be
/// preserved exactly when passing thinking blocks back to Anthropic in
/// multi-turn tool-use conversations. Modifying or omitting the signature
/// causes a 400 `invalid_request_error`.
///
/// OpenAI does not expose thinking/reasoning blocks in the same way;
/// reasoning tokens are invisible to the API consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningContent {
    /// The reasoning text content.
    pub thinking: String,
    /// Opaque signature required by Anthropic for multi-turn preservation.
    /// Must be passed back unmodified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// Source of an image — either a URL or inline base64 data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Image accessible via URL.
    Url {
        /// The image URL.
        url: String,
        /// Optional detail level (e.g., "auto", "low", "high" in OpenAI).
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Inline base64-encoded image.
    Base64 {
        /// MIME type of the image (e.g., "image/png").
        media_type: String,
        /// Base64-encoded image data.
        data: String,
    },
}

/// A tool call initiated by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseBlock {
    /// Unique identifier for this tool call (assigned by the model).
    pub id: String,
    /// Name of the tool to call.
    pub name: String,
    /// Arguments to pass to the tool (JSON object).
    pub input: serde_json::Value,
}

/// The result of a tool call, provided by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultBlock {
    /// The tool call ID this result corresponds to.
    pub tool_use_id: String,
    /// Name of the tool that was called (optional, for logging/debugging).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Content of the tool result.
    pub content: ToolResultContent,
    /// Whether the tool call resulted in an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Content of a tool result — either text or structured content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Simple text result.
    Text(String),
    /// Structured content blocks.
    Blocks(Vec<ContentBlock>),
}

/// A deferred tool reference — not fully materialized into a schema.
///
/// Tool discovery mechanisms can expose references rather than full schemas.
/// These must not be flattened accidentally during translation. The adapter
/// pipeline must distinguish fully-loaded tools from deferred references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReference {
    /// Unique identifier for this tool reference.
    pub id: String,
    /// Name of the tool.
    pub name: String,
    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional JSON schema for the tool's input (absent if deferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// Whether this reference is deferred (not fully loaded).
    #[serde(default = "default_true")]
    pub deferred: bool,
}

fn default_true() -> bool {
    true
}

// ─── Tool definitions ────────────────────────────────────────────────────────

/// A tool definition included in a request, telling the model what tools are available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool.
    pub name: String,
    /// Human-readable description of what the tool does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema describing the tool's input parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// Whether this is a deferred reference (not fully loaded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deferred: Option<bool>,
    /// Provider-specific tool metadata.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ─── Tool choice ─────────────────────────────────────────────────────────────

/// Controls how the model selects tool calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    /// Model decides whether to call tools.
    Auto,
    /// Model must call a tool.
    Required,
    /// Model must not call any tools.
    None,
    /// Model must call a specific tool by name.
    Named { name: String },
}

// ─── Messages ────────────────────────────────────────────────────────────────

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender.
    pub role: Role,
    /// Content of the message.
    pub content: MessageContent,
}

/// Content of a message — either a single string or structured content blocks.
///
/// In the canonical model, we normalize to content blocks internally.
/// Single-string content is represented as a single `TextContent` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content (most common).
    Text(String),
    /// Structured content blocks.
    Blocks(Vec<ContentBlock>),
}

impl MessageContent {
    /// Create text-only content.
    pub fn text(s: impl Into<String>) -> Self {
        MessageContent::Text(s.into())
    }

    /// Create structured content from blocks.
    pub fn blocks(blocks: Vec<ContentBlock>) -> Self {
        MessageContent::Blocks(blocks)
    }

    /// Normalize to content blocks.
    pub fn into_blocks(self) -> Vec<ContentBlock> {
        match self {
            MessageContent::Text(s) => vec![ContentBlock::Text(TextContent { text: s })],
            MessageContent::Blocks(blocks) => blocks,
        }
    }

    /// Check if content is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            MessageContent::Text(s) => s.is_empty(),
            MessageContent::Blocks(blocks) => blocks.is_empty(),
        }
    }
}

// ─── System instructions ─────────────────────────────────────────────────────

/// System-level instructions. In the canonical model, system instructions can
/// be either a single string or an array of content blocks (for providers that
/// support structured system prompts).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemInstruction {
    /// Simple text instruction.
    Text(String),
    /// Structured instruction blocks.
    Blocks(Vec<SystemBlock>),
}

/// A single block in a system instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    /// Type of the block (currently only "text").
    #[serde(rename = "type")]
    pub block_type: String,
    /// Text content of the block.
    pub text: String,
}

// ─── Request ─────────────────────────────────────────────────────────────────

/// A canonical LLM request. This is the provider-agnostic representation of
/// everything a client sends to an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRequest {
    /// The target model (provider-specific model identifier).
    pub model: String,
    /// Messages in the conversation.
    pub messages: Vec<Message>,
    /// Optional system instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemInstruction>,
    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// nucleus sampling threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    /// Available tools.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    /// Tool selection strategy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Whether to stream the response.
    #[serde(default)]
    pub stream: bool,
    /// Response format constraints (e.g., JSON mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Optional metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Provider-specific extensions.
    #[serde(flatten)]
    pub extensions: ProviderExtensions,
}

/// Response format constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    /// Format type (e.g., "text", "json_object", "json_schema").
    #[serde(rename = "type")]
    pub format_type: String,
    /// JSON schema (when format_type is "json_schema").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

// ─── Usage ───────────────────────────────────────────────────────────────────

/// Token usage information. Captures the union of token count fields across
/// providers. Not every field is populated for every response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt/input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    /// Tokens generated as output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    /// Total tokens (some providers report this instead of the sum).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
    /// Tokens that hit the cache (Anthropic-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Tokens served from cache (Anthropic-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

// ─── Finish reasons ──────────────────────────────────────────────────────────

/// Why the model stopped generating. Maps between provider-specific stop reasons.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Model completed its response naturally.
    Stop,
    /// Hit a stop sequence.
    StopSequence,
    /// Reached the token limit.
    Length,
    /// Model wants to call a tool.
    ToolCalls,
    /// Content was filtered/modified by safety systems.
    ContentFilter,
    /// Model hit an error.
    Error,
    /// Provider-specific finish reason (preserved but not silently acted upon).
    Other(String),
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FinishReason::Stop => write!(f, "stop"),
            FinishReason::StopSequence => write!(f, "stop_sequence"),
            FinishReason::Length => write!(f, "length"),
            FinishReason::ToolCalls => write!(f, "tool_calls"),
            FinishReason::ContentFilter => write!(f, "content_filter"),
            FinishReason::Error => write!(f, "error"),
            FinishReason::Other(s) => write!(f, "{s}"),
        }
    }
}

// ─── Response ────────────────────────────────────────────────────────────────

/// A canonical LLM response. Provider-agnostic representation of a complete
/// (non-streaming) LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalResponse {
    /// Unique identifier for this response.
    pub id: String,
    /// The model that generated this response.
    pub model: String,
    /// Response content blocks.
    pub content: Vec<ContentBlock>,
    /// Why the model stopped generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Token usage information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Provider-specific extensions.
    #[serde(flatten)]
    pub extensions: ProviderExtensions,
}

// ─── Provider extensions ─────────────────────────────────────────────────────

/// Provider-specific extensions. Typed where practical, stored as JSON values
/// where the schema is provider-specific. These preserve information that would
/// otherwise be lost in translation.
///
/// ```text
/// extensions:
///   anthropic: { ... }
///   openai: { ... }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderExtensions {
    /// OpenAI-specific extensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<serde_json::Value>,
    /// Anthropic-specific extensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<serde_json::Value>,
}

// ─── Capability matrix ───────────────────────────────────────────────────────

/// Declares what a protocol adapter supports. Used for capability negotiation
/// and to identify incompatible translations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolCapabilities {
    /// Supports streaming responses.
    pub streaming: bool,
    /// Supports tool/function calling.
    pub tools: bool,
    /// Supports streaming tool call deltas.
    pub tool_streaming: bool,
    /// Supports multimodal input (images, audio).
    pub multimodal_input: bool,
    /// Supports structured output / JSON mode.
    pub structured_output: bool,
    /// Supports reasoning/thinking tokens.
    pub reasoning: bool,
    /// Supports usage information in streaming events.
    pub usage_streaming: bool,
    /// Supports deferred tool references (not fully loaded schemas).
    pub deferred_tools: bool,
}

impl ProtocolCapabilities {
    /// Check if translation from `source` to `self` (target) is lossless
    /// for the given feature set. Returns a list of lossy translations.
    pub fn translation_losses(&self, source: &ProtocolCapabilities) -> Vec<LossyTranslation> {
        let mut losses = Vec::new();

        if source.streaming && !self.streaming {
            losses.push(LossyTranslation {
                feature: "streaming".into(),
                policy: LossPolicy::Reject,
                reason: "target protocol does not support streaming; a streaming \
                         request cannot be relayed"
                    .into(),
            });
        }
        if source.tools && !self.tools {
            losses.push(LossyTranslation {
                feature: "tools".into(),
                policy: LossPolicy::Reject,
                reason: "target protocol does not support tools; tool calls cannot \
                         be preserved"
                    .into(),
            });
        }
        if source.tool_streaming && !self.tool_streaming {
            losses.push(LossyTranslation {
                feature: "tool_call_streaming".into(),
                policy: LossPolicy::Approximate,
                reason: "target does not support incremental tool call deltas; \
                         full tool call text will be buffered before sending"
                    .into(),
            });
        }
        if source.multimodal_input && !self.multimodal_input {
            losses.push(LossyTranslation {
                feature: "multimodal_input".into(),
                policy: LossPolicy::Drop,
                reason: "target protocol does not support multimodal input; \
                         image/audio content blocks would be lost"
                    .into(),
            });
        }
        if source.structured_output && !self.structured_output {
            losses.push(LossyTranslation {
                feature: "structured_output".into(),
                policy: LossPolicy::Drop,
                reason: "target does not support structured output format constraints".into(),
            });
        }
        if source.reasoning && !self.reasoning {
            losses.push(LossyTranslation {
                feature: "reasoning".into(),
                policy: LossPolicy::EncodeAsExtension,
                reason: "target does not support reasoning tokens; \
                         reasoning content will be included as extension metadata"
                    .into(),
            });
        }
        if source.usage_streaming && !self.usage_streaming {
            losses.push(LossyTranslation {
                feature: "usage_streaming".into(),
                policy: LossPolicy::Drop,
                reason: "target does not report usage in streaming events; \
                         streaming usage counts will be lost"
                    .into(),
            });
        }
        if source.deferred_tools && !self.deferred_tools {
            losses.push(LossyTranslation {
                feature: "deferred_tools".into(),
                policy: LossPolicy::Approximate,
                reason: "target does not support deferred tool references; \
                         tool definitions will be fully materialized"
                    .into(),
            });
        }

        losses
    }

    /// Check if translation from `source` to `self` (target) would result in
    /// a rejected or dropped feature. Returns the first such loss as an error,
    /// or `Ok(())` if translation is lossless or only approximated.
    ///
    /// Use this at adapter encode boundaries to prevent silent capability loss.
    pub fn enforce_translation_losses(
        &self,
        source: &ProtocolCapabilities,
    ) -> Result<(), crate::error::ProtocolEngineError> {
        let losses = self.translation_losses(source);
        for loss in &losses {
            match loss.policy {
                LossPolicy::Reject | LossPolicy::Drop => {
                    return Err(crate::error::ProtocolEngineError::LossyTranslation {
                        feature: loss.feature.clone(),
                        reason: loss.reason.clone(),
                        policy: loss.policy.clone(),
                    });
                }
                LossPolicy::Approximate | LossPolicy::Warn | LossPolicy::EncodeAsExtension => {
                    tracing::warn!(
                        feature = %loss.feature,
                        policy = ?loss.policy,
                        reason = %loss.reason,
                        "lossy translation accepted (non-rejected policy)"
                    );
                }
            }
        }
        Ok(())
    }
}

/// Describes a single potentially lossy translation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LossyTranslation {
    /// The feature being lost.
    pub feature: String,
    /// How the loss is handled.
    pub policy: LossPolicy,
    /// Human-readable explanation.
    pub reason: String,
}

/// Policy for handling a feature that the target protocol cannot represent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LossPolicy {
    /// Reject the translation entirely.
    Reject,
    /// Warn but proceed.
    Warn,
    /// Silently drop the feature.
    Drop,
    /// Best-effort approximation.
    Approximate,
    /// Encode in an extension field.
    EncodeAsExtension,
}

// ─── Stream events ───────────────────────────────────────────────────────────

/// A canonical streaming event. Represents one discrete event in a streaming
/// response. The event taxonomy follows the union of capabilities from all
/// supported protocols.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanonicalStreamEvent {
    /// Streaming response is starting.
    MessageStart {
        /// Response metadata (id, model).
        message: StreamMessageInfo,
    },
    /// A new content block is starting (e.g., text or tool_use).
    ContentBlockStart {
        /// Index of this content block.
        index: usize,
        /// The content block being started.
        content_block: ContentBlock,
    },
    /// Incremental text delta.
    TextDelta {
        /// Index of the content block this delta belongs to.
        index: usize,
        /// The text delta.
        text: String,
    },
    /// Incremental audio data delta (base64-encoded chunk).
    AudioDelta {
        /// Index of the content block this delta belongs to.
        index: usize,
        /// Base64-encoded audio chunk.
        data: String,
    },
    /// Incremental reasoning/thinking delta.
    ReasoningDelta {
        /// Index of the content block this delta belongs to.
        index: usize,
        /// The reasoning text delta.
        thinking: String,
    },
    /// Opaque signature for a reasoning block (Anthropic-specific).
    ///
    /// Delivered just before `ContentBlockStop` for a reasoning block.
    /// Must be preserved exactly for multi-turn tool-use continuation.
    ReasoningSignature {
        /// Index of the content block this signature belongs to.
        index: usize,
        /// The opaque signature.
        signature: String,
    },
    /// Incremental tool call name (may arrive in chunks).
    ToolCallDelta {
        /// Index of the content block this delta belongs to.
        index: usize,
        /// Tool call ID (present in first delta).
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
        /// Tool name (present in first delta).
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Partial JSON arguments delta.
        #[serde(skip_serializing_if = "Option::is_none")]
        input_json_delta: Option<String>,
    },
    /// A content block has ended.
    ContentBlockStop {
        /// Index of the content block that ended.
        index: usize,
    },
    /// Updated message-level metadata (stop reason, usage delta).
    MessageDelta {
        /// Updated stop reason.
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<FinishReason>,
        /// Updated usage information.
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    /// Cumulative usage information.
    Usage {
        /// The usage data.
        usage: Usage,
    },
    /// The streaming response has ended.
    MessageStop,
    /// An error occurred during streaming.
    Error {
        /// Error message.
        message: String,
        /// Error code.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// Ping event (Anthropic uses this to keep connections alive).
    Ping,
}

/// Metadata about the message being streamed, provided at stream start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMessageInfo {
    /// Unique identifier for this response.
    pub id: String,
    /// The model generating this response.
    pub model: String,
    /// The role (always "assistant" for responses).
    pub role: Role,
}

// ─── Error model ─────────────────────────────────────────────────────────────

// ─── End of canonical model ─────────────────────────────────────────────────
