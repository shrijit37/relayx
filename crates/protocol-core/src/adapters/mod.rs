//! Protocol adapters.
//!
//! Each adapter translates between a specific wire protocol and the canonical
//! model. Adapters are stateless — they receive a request/response and return
//! the translated form. No adapter holds connection state or configuration.

pub mod anthropic_messages;
pub mod openai_chat;
pub mod openai_responses;
