use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::chat::{
    CreateChatConversationRequest, CreateChatConversationStreamRequest, CreateChatMessageRequest,
    UpdateChatConversationRequest,
};
use crate::dto::common::PaginationQuery;
use crate::error::AppError;
use crate::extract::{VJson, VPath, VQuery};
use crate::models::chat::{ChatConversationResponse, ChatMessageResponse};
use crate::response::ApiResponse;
use crate::services::chat::StreamOutcome;
use crate::services::{self, chat::ChatError, openrouter};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::StreamExt;
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
        ChatError::ConversationNotOwned => {
            AppError::Forbidden("You do not own this conversation".to_string())
        }
        ChatError::MessageNotFound => AppError::NotFound("Chat message not found".to_string()),
    }
}

pub async fn create_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<CreateChatConversationRequest>,
) -> Result<(StatusCode, Json<ApiResponse<ChatConversationResponse>>), AppError> {
    let conversation = services::chat::create_conversation(&pool, auth_user.id, req)
        .await
        .map_err(map_chat_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Successfully created conversation",
            conversation,
        )),
    ))
}

/// `POST /api/chat/conversations/stream` — creates a conversation plus its
/// first message; streams the AI response over SSE when AI is available,
/// otherwise falls back to `201` with the user message (mirrors echobackend's
/// `CreateConversationStream`).
pub async fn create_conversation_stream(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<CreateChatConversationStreamRequest>,
) -> Result<Response, AppError> {
    match services::chat::create_conversation_stream(&pool, auth_user.id, req)
        .await
        .map_err(map_chat_error)?
    {
        StreamOutcome::Fallback(user_message) => Ok((
            StatusCode::CREATED,
            Json(ApiResponse::success_with_message(
                "Message created successfully",
                vec![user_message],
            )),
        )
            .into_response()),
        StreamOutcome::Stream(preparation) => {
            let initial_event = serde_json::json!({
                "type": "conversation_created",
                "data": {
                    "conversation_id": preparation.conversation_id,
                    "user_message": preparation.user_message,
                },
            });
            Ok(ai_stream_response(pool, preparation, initial_event).into_response())
        }
    }
}

pub async fn get_conversations(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<ChatConversationResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
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
    VPath(params): VPath<ConversationPath>,
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
    VPath(params): VPath<ConversationPath>,
    VJson(req): VJson<UpdateChatConversationRequest>,
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
    VPath(params): VPath<ConversationPath>,
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
    VPath(params): VPath<ConversationMessagesPath>,
    VJson(req): VJson<CreateChatMessageRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Vec<ChatMessageResponse>>>), AppError> {
    let messages = services::chat::create_message(&pool, auth_user.id, params.conversation_id, req)
        .await
        .map_err(map_chat_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Messages created successfully",
            messages,
        )),
    ))
}

/// `POST /api/chat/conversations/:conversationId/messages/stream` — stores the
/// user message and streams the AI response over SSE when available,
/// otherwise falls back to `201` with the user message (mirrors
/// echobackend's `CreateMessageStream`).
pub async fn create_message_stream(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<ConversationMessagesPath>,
    VJson(req): VJson<CreateChatMessageRequest>,
) -> Result<Response, AppError> {
    match services::chat::prepare_streaming_message(
        &pool,
        auth_user.id,
        params.conversation_id,
        req,
    )
    .await
    .map_err(map_chat_error)?
    {
        StreamOutcome::Fallback(user_message) => Ok((
            StatusCode::CREATED,
            Json(ApiResponse::success_with_message(
                "Message created successfully",
                vec![user_message],
            )),
        )
            .into_response()),
        StreamOutcome::Stream(preparation) => {
            let initial_event = serde_json::json!({
                "type": "user_message",
                "data": preparation.user_message,
            });
            Ok(ai_stream_response(pool, preparation, initial_event).into_response())
        }
    }
}

pub async fn get_messages(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<ConversationMessagesPath>,
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
    VPath(params): VPath<MessagePath>,
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
    VPath(params): VPath<MessagePath>,
) -> Result<Json<ApiResponse<ChatMessageResponse>>, AppError> {
    let message = services::chat::delete_message(&pool, params.message_id, auth_user.id)
        .await
        .map_err(map_chat_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Message deleted successfully",
        message,
    )))
}

// ============================================================================
// SSE streaming
// ============================================================================

fn sse_data_event(payload: serde_json::Value) -> Result<Event, Infallible> {
    Ok(Event::default().data(payload.to_string()))
}

fn sse_error_event(message: &str) -> Result<Event, Infallible> {
    sse_data_event(serde_json::json!({ "type": "error", "data": message }))
}

/// Builds the SSE response for a prepared AI stream, mirroring echobackend's
/// `streamChatEvents`: the initial event (`conversation_created` /
/// `user_message`), `ai_chunk` events, `ai_complete` (or `error`), and a
/// final `data: [DONE]`.
fn ai_stream_response(
    pool: DbPool,
    preparation: services::chat::StreamPreparation,
    initial_event: serde_json::Value,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        yield sse_data_event(initial_event);

        match openrouter::generate_stream(
            &preparation.context_messages,
            preparation.model.as_deref(),
            preparation.temperature,
        )
        .await
        {
            Err(err) => {
                yield sse_error_event(&openrouter::stream_error_message(&err));
            }
            Ok(events) => {
                tokio::pin!(events);
                let mut content = String::new();
                let mut usage = openrouter::OpenRouterUsage::default();
                let mut failed: Option<String> = None;

                while let Some(event) = events.next().await {
                    match event {
                        openrouter::StreamEvent::Chunk(chunk) => {
                            content.push_str(&chunk);
                            yield sse_data_event(
                                serde_json::json!({ "type": "ai_chunk", "data": chunk }),
                            );
                        }
                        openrouter::StreamEvent::Usage(value) => usage = value,
                        openrouter::StreamEvent::Error(err) => {
                            failed = Some(err);
                            break;
                        }
                    }
                }

                if let Some(err) = failed {
                    yield sse_error_event(&openrouter::stream_error_message(&err));
                } else if content.is_empty() {
                    yield sse_error_event("OpenRouter returned an empty response");
                } else {
                    match services::chat::save_streaming_message(
                        &pool,
                        preparation.conversation_id,
                        preparation.user_id,
                        content,
                        preparation.model.clone(),
                        usage,
                    )
                    .await
                    {
                        Ok(message) => {
                            yield sse_data_event(
                                serde_json::json!({ "type": "ai_complete", "data": message }),
                            );
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, "failed to save streamed AI message");
                            yield sse_error_event("Failed to save AI response");
                        }
                    }
                }
            }
        }

        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(stream)
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
