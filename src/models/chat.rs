use crate::entities::{chat_conversations, chat_messages};
use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct ChatMessageResponse {
    pub id: String,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    pub conversation_id: String,
    pub user_id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

#[derive(Debug, Serialize)]
pub struct ChatConversationResponse {
    pub id: String,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    pub title: String,
    pub is_pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<DateTime<FixedOffset>>,
    pub user_id: String,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_messages: Option<Vec<ChatMessageResponse>>,
}

/// One SSE `data:` line of the chat AI stream (echobackend
/// `streamChatEvents`): `{ "type": ..., "data": ... }` where `type` is
/// `conversation_created`, `user_message`, `ai_chunk`, `ai_complete` or
/// `error`.
#[derive(Debug, Serialize)]
pub struct ChatStreamEvent<T> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub data: T,
}

/// `data` of the `conversation_created` stream event.
#[derive(Debug, Serialize)]
pub struct ChatConversationCreatedData<'a> {
    pub conversation_id: Uuid,
    pub user_message: &'a ChatMessageResponse,
}

pub fn message_response(message: chat_messages::Model) -> ChatMessageResponse {
    ChatMessageResponse {
        id: message.id.to_string(),
        created_at: message.created_at,
        updated_at: message.updated_at,
        conversation_id: message.conversation_id.to_string(),
        user_id: message.user_id.to_string(),
        role: message.role,
        content: message.content,
        model: message.model,
        prompt_tokens: message.prompt_tokens.unwrap_or_default(),
        completion_tokens: message.completion_tokens.unwrap_or_default(),
        total_tokens: message.total_tokens.unwrap_or_default(),
    }
}

pub fn conversation_response(
    conversation: chat_conversations::Model,
    messages: Option<Vec<chat_messages::Model>>,
    message_count: usize,
) -> ChatConversationResponse {
    ChatConversationResponse {
        id: conversation.id.to_string(),
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
        title: conversation.title,
        is_pinned: conversation.is_pinned,
        pinned_at: conversation.pinned_at,
        user_id: conversation.user_id.to_string(),
        message_count,
        chat_messages: messages.map(|items| items.into_iter().map(message_response).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_message() -> ChatMessageResponse {
        let ts = DateTime::parse_from_rfc3339("2026-01-02T03:04:05.123456+00:00").unwrap();
        ChatMessageResponse {
            id: "8a0d6c7e-1111-4222-8333-944455556666".into(),
            created_at: ts,
            updated_at: ts,
            conversation_id: "0c3f1d2e-aaaa-4bbb-8ccc-dddddddddddd".into(),
            user_id: "11111111-2222-4333-8444-555555555555".into(),
            role: "user".into(),
            content: "hello".into(),
            model: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }
    }

    #[test]
    fn conversation_created_event_shape() {
        let conversation_id = Uuid::parse_str("0c3f1d2e-aaaa-4bbb-8ccc-dddddddddddd").unwrap();
        let user_message = sample_message();
        let event = ChatStreamEvent {
            kind: "conversation_created",
            data: ChatConversationCreatedData {
                conversation_id,
                user_message: &user_message,
            },
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "conversation_created",
                "data": {
                    "conversation_id": conversation_id,
                    "user_message": user_message,
                },
            })
        );
    }

    #[test]
    fn user_message_event_shape() {
        let user_message = sample_message();
        let event = ChatStreamEvent {
            kind: "user_message",
            data: &user_message,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({ "type": "user_message", "data": user_message })
        );
    }

    #[test]
    fn ai_chunk_event_shape() {
        let chunk = "partial \"text\"\n".to_string();
        let event = ChatStreamEvent {
            kind: "ai_chunk",
            data: &chunk,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({ "type": "ai_chunk", "data": chunk })
        );
    }

    #[test]
    fn ai_complete_event_shape() {
        let mut message = sample_message();
        message.role = "assistant".into();
        message.model = Some("openrouter/free".into());
        message.total_tokens = 12;
        let event = ChatStreamEvent {
            kind: "ai_complete",
            data: &message,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({ "type": "ai_complete", "data": message })
        );
    }

    #[test]
    fn error_event_shape() {
        let message = "OpenRouter returned an empty response";
        let event = ChatStreamEvent {
            kind: "error",
            data: message,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({ "type": "error", "data": message })
        );
    }
}
