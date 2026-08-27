use crate::entities::{chat_conversations, chat_messages};
use chrono::{DateTime, FixedOffset};
use serde::Serialize;

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
