use crate::clients::openrouter;
use crate::dto::chat::{
    CreateChatConversationRequest, CreateChatConversationStreamRequest, CreateChatMessageRequest,
    UpdateChatConversationRequest,
};
use crate::dto::common::PaginationQuery;
use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use crate::http::extract::{VBind, VQuery, bind_message, canonical_uuid_param};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::chat::{
    ChatConversationCreatedData, ChatConversationResponse, ChatMessageResponse, ChatStreamEvent,
};
use crate::services::chat::StreamOutcome;
use crate::services::{self, chat::ChatError};
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use serde::Serialize;
use std::convert::Infallible;
use tokio_stream::StreamExt;

/// echobackend `respondChatError(c, message, err)`: 404 carries the endpoint
/// message plus the sentinel text as `error`, 403 uses the sentinel as its
/// message, anything else is a 500 with the endpoint message.
fn chat_err(message: &'static str) -> impl FnOnce(ChatError) -> AppError {
    move |err| match err {
        ChatError::Db(err) => AppError::from(err).context(message),
        ChatError::ConversationNotFound => {
            AppError::not_found(message, Some("chat conversation not found".into()))
        }
        ChatError::MessageNotFound => {
            AppError::not_found(message, Some("chat message not found".into()))
        }
        ChatError::ConversationNotOwned => {
            AppError::Forbidden("access denied: conversation does not belong to user".to_string())
        }
    }
}

/// echobackend `MsgInvalidConversationID`: the `{id}` / `{conversationId}`
/// segment must be a canonical UUID, checked before the body and any DB access.
pub struct ConversationIdParam(pub uuid::Uuid);

impl<S: Send + Sync> FromRequestParts<S> for ConversationIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let mut id = canonical_uuid_param(parts, state, "id").await;
        if id.is_none() {
            id = canonical_uuid_param(parts, state, "conversationId").await;
        }
        id.map(ConversationIdParam)
            .ok_or_else(|| AppError::bad_request("Invalid conversation ID", None))
    }
}

/// echobackend `MsgInvalidMessageID` for the `{messageId}` segment.
pub struct MessageIdParam(pub uuid::Uuid);

impl<S: Send + Sync> FromRequestParts<S> for MessageIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        canonical_uuid_param(parts, state, "messageId")
            .await
            .map(MessageIdParam)
            .ok_or_else(|| AppError::bad_request("Invalid message ID", None))
    }
}

/// echobackend `c.Bind` + `c.Validate` with the endpoint's own bind-failure
/// message (400 only; 413/422 are left as they are).
fn bind_body<T>(body: Result<VBind<T>, AppError>, message: &'static str) -> Result<T, AppError> {
    body.map(|VBind(value)| value)
        .map_err(bind_message(message))
}

pub async fn create_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    body: Result<VBind<CreateChatConversationRequest>, AppError>,
) -> Result<(StatusCode, Json<ApiResponse<ChatConversationResponse>>), AppError> {
    let req = bind_body(body, "Failed to create conversation")?;
    let conversation = services::chat::create_conversation(&pool, auth_user.id, req)
        .await
        .map_err(chat_err("Failed to create conversation"))?;
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
    body: Result<VBind<CreateChatConversationStreamRequest>, AppError>,
) -> Result<Response, AppError> {
    let req = bind_body(body, "Failed to create conversation stream")?;
    match services::chat::create_conversation_stream(&pool, auth_user.id, req)
        .await
        .map_err(chat_err("Failed to create conversation stream"))?
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
            let initial_event = sse_data_event(
                "conversation_created",
                ChatConversationCreatedData {
                    conversation_id: preparation.conversation_id,
                    user_message: &preparation.user_message,
                },
            );
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
            .map_err(chat_err("Failed to get conversations"))?;
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
    ConversationIdParam(id): ConversationIdParam,
) -> Result<Json<ApiResponse<ChatConversationResponse>>, AppError> {
    let conversation = services::chat::get_conversation_by_id(&pool, id, auth_user.id)
        .await
        .map_err(chat_err("Failed to get conversation"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved conversation",
        conversation,
    )))
}

pub async fn update_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    ConversationIdParam(id): ConversationIdParam,
    body: Result<VBind<UpdateChatConversationRequest>, AppError>,
) -> Result<Json<ApiResponse<ChatConversationResponse>>, AppError> {
    let req = bind_body(body, "Failed to update conversation")?;
    let conversation = services::chat::update_conversation(&pool, id, auth_user.id, req)
        .await
        .map_err(chat_err("Failed to update conversation"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Conversation updated successfully",
        conversation,
    )))
}

pub async fn delete_conversation(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    ConversationIdParam(id): ConversationIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::chat::delete_conversation(&pool, id, auth_user.id)
        .await
        .map_err(chat_err("Failed to delete conversation"))?;
    Ok(Json(ApiResponse::message(
        "Successfully deleted conversation",
    )))
}

pub async fn create_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    ConversationIdParam(conversation_id): ConversationIdParam,
    body: Result<VBind<CreateChatMessageRequest>, AppError>,
) -> Result<(StatusCode, Json<ApiResponse<Vec<ChatMessageResponse>>>), AppError> {
    let req = bind_body(body, "Failed to create message")?;
    let messages = services::chat::create_message(&pool, auth_user.id, conversation_id, req)
        .await
        .map_err(chat_err("Failed to create message"))?;
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
    ConversationIdParam(conversation_id): ConversationIdParam,
    body: Result<VBind<CreateChatMessageRequest>, AppError>,
) -> Result<Response, AppError> {
    let req = bind_body(body, "Failed to create message stream")?;
    match services::chat::prepare_streaming_message(&pool, auth_user.id, conversation_id, req)
        .await
        .map_err(chat_err("Failed to create message stream"))?
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
            let initial_event = sse_data_event("user_message", &preparation.user_message);
            Ok(ai_stream_response(pool, preparation, initial_event).into_response())
        }
    }
}

pub async fn get_messages(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    ConversationIdParam(conversation_id): ConversationIdParam,
) -> Result<Json<ApiResponse<Vec<ChatMessageResponse>>>, AppError> {
    let messages = services::chat::get_messages(&pool, conversation_id, auth_user.id)
        .await
        .map_err(chat_err("Failed to get messages"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Messages fetched successfully",
        messages,
    )))
}

pub async fn get_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    MessageIdParam(message_id): MessageIdParam,
) -> Result<Json<ApiResponse<ChatMessageResponse>>, AppError> {
    let message = services::chat::get_message(&pool, message_id, auth_user.id)
        .await
        .map_err(chat_err("Failed to get message"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Message fetched successfully",
        message,
    )))
}

pub async fn delete_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    MessageIdParam(message_id): MessageIdParam,
) -> Result<Json<ApiResponse<ChatMessageResponse>>, AppError> {
    let message = services::chat::delete_message(&pool, message_id, auth_user.id)
        .await
        .map_err(chat_err("Failed to delete message"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Message deleted successfully",
        message,
    )))
}

// ============================================================================
// SSE streaming
// ============================================================================

/// Serializes one `{ "type": kind, "data": data }` stream event into a
/// `data:` line. The payloads (strings, `ChatMessageResponse`) always
/// serialize, so the empty fallback is unreachable in practice.
pub(super) fn sse_data_event<T: Serialize>(
    kind: &'static str,
    data: T,
) -> Result<Event, Infallible> {
    let payload = serde_json::to_string(&ChatStreamEvent { kind, data }).unwrap_or_default();
    Ok(Event::default().data(payload))
}

pub(super) fn sse_error_event(message: &str) -> Result<Event, Infallible> {
    sse_data_event("error", message)
}

/// Builds the SSE response for a prepared AI stream, mirroring echobackend's
/// `streamChatEvents`: the initial event (`conversation_created` /
/// `user_message`), `ai_chunk` events, `ai_complete` (or `error`), and a
/// final `data: [DONE]`.
fn ai_stream_response(
    pool: DbPool,
    preparation: services::chat::StreamPreparation,
    initial_event: Result<Event, Infallible>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        yield initial_event;

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
                            yield sse_data_event("ai_chunk", &chunk);
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
                            yield sse_data_event("ai_complete", &message);
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

pub fn routes() -> Router<AppState> {
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
