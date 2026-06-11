//! Client-facing ingress dialects. Each module is the inverse of an egress
//! transport's wire mapping, but on the CLIENT side: parse a dialect request body
//! into the unified `ChatRequest`, and serialize a unified `ChatResponse`/
//! `StreamDelta` back out into that dialect (incl. its SSE frame shape). P1.4
//! routes each `/v1/...` endpoint to its dialect here.

pub mod anthropic_messages;
pub mod openai_chat;
pub mod openai_responses;
