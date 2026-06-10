use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::chat::{
    CreateChatConversationRequest, CreateChatConversationStreamRequest, CreateChatMessageRequest,
    UpdateChatConversationRequest,
};
use crate::dto::common::PaginationQuery;
use crate::error::AppError;
use crate::models::chat::{ChatConversationResponse, ChatMessageResponse, ChatStreamResult};
use crate::response::ApiResponse;
use crate::services::{self, chat::ChatError};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use axum_valid::Valid;
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct ConversationPath {
    pub id: Uuid,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessagesPath {
    pub conversation_id: Uuid,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct MessagePath {
    pub message_id: Uuid,
}

fn map_chat_error(err: ChatError) -> AppError {
    match err {
        ChatError::Db(err) => AppError::from(err),
        ChatError::ConversationNotFound => {
            AppError::NotFound("Chat conversation not found".to_string())
        }
        ChatError::MessageNotFound => AppError::NotFound("Chat message not found".to_string()),
    }
}

pub async fn create_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Json(req)): Valid<Json<CreateChatConversationRequest>>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<ChatConversationResponse>>,
    ),
    AppError,
> {
    let conversation = services::chat::create_conversation(&pool, auth_user.id, req)
        .await
        .map_err(map_chat_error)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Successfully created conversation",
            conversation,
        )),
    ))
}

pub async fn create_conversation_stream(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Json(req)): Valid<Json<CreateChatConversationStreamRequest>>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<Vec<ChatStreamResult>>>,
    ),
    AppError,
> {
    let result = services::chat::create_conversation_message(&pool, auth_user.id, req)
        .await
        .map_err(map_chat_error)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Message created successfully",
            vec![result],
        )),
    ))
}

pub async fn get_conversations(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<PaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<ChatConversationResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let (conversations, total) =
        services::chat::get_user_conversations(&pool, auth_user.id, offset as u64, limit as u64)
            .await
            .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved conversations",
        conversations,
        total,
        limit,
        offset,
    )))
}

pub async fn get_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<ConversationPath>>,
) -> Result<Json<ApiResponse<ChatConversationResponse>>, AppError> {
    let conversation = services::chat::get_conversation_by_id(&pool, params.id, auth_user.id)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved conversation",
        conversation,
    )))
}

pub async fn update_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<ConversationPath>>,
    Valid(Json(req)): Valid<Json<UpdateChatConversationRequest>>,
) -> Result<Json<ApiResponse<ChatConversationResponse>>, AppError> {
    let conversation = services::chat::update_conversation(&pool, params.id, auth_user.id, req)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Conversation updated successfully",
        conversation,
    )))
}

pub async fn delete_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<ConversationPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::chat::delete_conversation(&pool, params.id, auth_user.id)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully deleted conversation",
        serde_json::Value::Null,
    )))
}

pub async fn create_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<ConversationMessagesPath>>,
    Valid(Json(req)): Valid<Json<CreateChatMessageRequest>>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<Vec<ChatMessageResponse>>>,
    ),
    AppError,
> {
    let messages = services::chat::create_message(&pool, auth_user.id, params.conversation_id, req)
        .await
        .map_err(map_chat_error)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Messages created successfully",
            messages,
        )),
    ))
}

pub async fn create_message_stream(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<ConversationMessagesPath>>,
    Valid(Json(req)): Valid<Json<CreateChatMessageRequest>>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<Vec<ChatMessageResponse>>>,
    ),
    AppError,
> {
    create_message(
        State(pool),
        auth_user,
        Valid(Path(params)),
        Valid(Json(req)),
    )
    .await
}

pub async fn get_messages(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<ConversationMessagesPath>>,
) -> Result<Json<ApiResponse<Vec<ChatMessageResponse>>>, AppError> {
    let messages = services::chat::get_messages(&pool, params.conversation_id, auth_user.id)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Messages fetched successfully",
        messages,
    )))
}

pub async fn get_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<MessagePath>>,
) -> Result<Json<ApiResponse<ChatMessageResponse>>, AppError> {
    let message = services::chat::get_message(&pool, params.message_id, auth_user.id)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Message fetched successfully",
        message,
    )))
}

pub async fn delete_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<MessagePath>>,
) -> Result<Json<ApiResponse<ChatMessageResponse>>, AppError> {
    let message = services::chat::delete_message(&pool, params.message_id, auth_user.id)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Message deleted successfully",
        message,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route(
            "/api/chat/conversations",
            post(create_conversation).get(get_conversations),
        )
        .route(
            "/api/chat/conversations/stream",
            post(create_conversation_stream),
        )
        .route(
            "/api/chat/conversations/{id}",
            get(get_conversation)
                .put(update_conversation)
                .delete(delete_conversation),
        )
        .route(
            "/api/chat/conversations/{conversationId}/messages",
            post(create_message).get(get_messages),
        )
        .route(
            "/api/chat/conversations/{conversationId}/messages/stream",
            post(create_message_stream),
        )
        .route(
            "/api/chat/messages/{messageId}",
            get(get_message).delete(delete_message),
        )
}
