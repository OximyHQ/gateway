//! Provider-agnostic chat messages. A message has a role and an ordered list of
//! content parts (text or image). Tool results ride as a dedicated role so every
//! dialect can map them; tool *calls* live on assistant messages (defined in
//! `toolcall.rs`). These types are the canonical shape ALL ingress dialects map
//! onto and ALL egress transports map out of — keep them minimal and total.

use serde::{Deserialize, Serialize};

use crate::toolcall::ToolCall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    /// A tool/function result being fed back to the model.
    Tool,
}

/// Source of image bytes: an external URL or inline base64 with a media type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    Url { url: String },
    Base64 { media_type: String, data: String },
}

/// One piece of a message body. Multimodal messages interleave these in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { source: ImageSource },
}

impl ContentPart {
    pub fn text(s: impl Into<String>) -> Self {
        ContentPart::Text { text: s.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Ordered content parts. May be empty for an assistant message that only
    /// emitted tool calls.
    pub content: Vec<ContentPart>,
    /// Tool calls emitted by an assistant turn. Empty for non-assistant roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `Role::Tool`: which tool call this message answers. `None` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Convenience constructor for a single-text-part message.
    pub fn text(role: Role, body: impl Into<String>) -> Self {
        Message {
            role,
            content: vec![ContentPart::text(body)],
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Concatenate all text parts (images ignored) — used by transports that
    /// flatten to a single string.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_constructor_makes_one_part() {
        let m = Message::text(Role::User, "hello");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.content.len(), 1);
        assert_eq!(m.text_content(), "hello");
        assert!(m.tool_calls.is_empty());
    }

    #[test]
    fn multimodal_text_content_skips_images() {
        let m = Message {
            role: Role::User,
            content: vec![
                ContentPart::text("look: "),
                ContentPart::Image {
                    source: ImageSource::Url {
                        url: "https://x/y.png".into(),
                    },
                },
                ContentPart::text("done"),
            ],
            tool_calls: Vec::new(),
            tool_call_id: None,
        };
        assert_eq!(m.text_content(), "look: done");
    }

    #[test]
    fn role_serializes_snake_case() {
        let j = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(j, "\"assistant\"");
    }

    #[test]
    fn content_part_tagged_repr_roundtrips() {
        let p = ContentPart::text("hi");
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["type"], "text");
        assert_eq!(j["text"], "hi");
        let back: ContentPart = serde_json::from_value(j).unwrap();
        assert_eq!(back, p);
    }
}
