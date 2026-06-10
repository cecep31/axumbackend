use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatConversationRequest {
    #[validate(length(min = 1, max = 255))]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatConversationStreamRequest {
    #[validate(length(min = 1, max = 255))]
    pub title: Option<String>,
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
    #[validate(length(max = 100))]
    pub model: Option<String>,
    #[validate(range(min = 0.0, max = 2.0))]
    pub temperature: Option<f64>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateChatConversationRequest {
    #[validate(length(max = 255))]
    pub title: Option<String>,
    pub is_pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatMessageRequest {
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
    #[validate(length(max = 20))]
    pub role: Option<String>,
    #[validate(length(max = 100))]
    pub model: Option<String>,
    #[validate(range(min = 0.0, max = 2.0))]
    pub temperature: Option<f64>,
}
