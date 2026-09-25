use crate::auth::{AuthUser, OptionalAuthUser};
use crate::database::DbPool;
use crate::dto::common::PaginationQuery;
use crate::dto::guild::{
    CreateGuildChannelRequest, CreateGuildMessageRequest, CreateGuildRequest, GuildChannelPath,
    GuildListQuery, GuildMessagePath, GuildMessagesQuery, GuildSlugPath, UpdateGuildChannelRequest,
    UpdateGuildMessageRequest, UpdateGuildRequest,
};
use crate::error::AppError;
use crate::extract::{VJson, VPath, VQuery};
use crate::models::guild::{
    GuildChannelResponse, GuildMemberResponse, GuildMessageResponse, GuildMessagesResponse,
    GuildResponse,
};
use crate::response::ApiResponse;
use crate::services::guild::{CreateGuildInput, GuildError, UpdateGuildInput};
use crate::services::{self, guild_channel::UpdateChannelInput};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderName, HeaderValue, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{delete, get, patch, post},
};
use std::convert::Infallible;
use std::time::Duration;

/// Keeps proxies from closing an idle stream and is also how often a
/// subscriber's membership is re-checked, so a member who leaves stops
/// receiving events within this interval.
const GUILD_STREAM_HEARTBEAT: Duration = Duration::from_secs(25);

/// Maps guild errors for the guild endpoints (echobackend's
/// `handleGuildError`).
fn map_guild_error(err: GuildError) -> AppError {
    match err {
        GuildError::Db(err) => AppError::from(err),
        GuildError::GuildNotFound => AppError::NotFound("Guild not found".to_string()),
        GuildError::NotOwned => AppError::Forbidden("Access forbidden".to_string()),
        GuildError::SlugExists => AppError::Conflict("Guild slug already taken".to_string()),
        GuildError::AlreadyMember => {
            AppError::Conflict("Already a member of this guild".to_string())
        }
        other => AppError::BadRequest(other.to_string()),
    }
}

/// Maps guild errors for the channel, message and stream endpoints
/// (echobackend's `handleGuildChannelError`). Unlike the guild endpoints, a
/// non-member is forbidden here rather than a bad request.
fn map_guild_channel_error(err: GuildError) -> AppError {
    match err {
        GuildError::Db(err) => AppError::from(err),
        GuildError::GuildNotFound => AppError::NotFound("Guild not found".to_string()),
        GuildError::ChannelNotFound => AppError::NotFound("Channel not found".to_string()),
        GuildError::MessageNotFound => AppError::NotFound("Message not found".to_string()),
        GuildError::NotOwned | GuildError::NotMember | GuildError::MessageNotOwned => {
            AppError::Forbidden("Access forbidden".to_string())
        }
        GuildError::ChannelNameExists => {
            AppError::Conflict("Channel name already taken".to_string())
        }
        other => AppError::BadRequest(other.to_string()),
    }
}

fn viewer_id(viewer: &OptionalAuthUser) -> Option<uuid::Uuid> {
    viewer.0.as_ref().map(|user| user.id)
}

pub async fn create_guild(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<CreateGuildRequest>,
) -> Result<(StatusCode, Json<ApiResponse<GuildResponse>>), AppError> {
    let guild = services::guild::create_guild(
        &pool,
        auth_user.id,
        CreateGuildInput {
            name: req.name,
            slug: req.slug,
            description: req.description,
            avatar_url: req.avatar_url,
            is_public: req.is_public,
        },
    )
    .await
    .map_err(map_guild_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Guild created successfully",
            guild,
        )),
    ))
}

/// The public guild directory. It is open to anonymous callers, so no
/// viewer-relative fields are filled in.
pub async fn list_guilds(
    State(pool): State<DbPool>,
    VQuery(query): VQuery<GuildListQuery>,
) -> Result<Json<ApiResponse<Vec<GuildResponse>>>, AppError> {
    let (limit, offset) = query.resolve();
    let (guilds, total) =
        services::guild::list_guilds(&pool, query.search.as_deref(), limit, offset)
            .await
            .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::with_meta_message(
        "Guilds fetched successfully",
        guilds,
        total,
        limit,
        offset,
    )))
}

/// The guilds the caller belongs to, including private ones.
pub async fn get_my_guilds(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<GuildResponse>>>, AppError> {
    let (limit, offset) = query.resolve(20);
    let (guilds, total) = services::guild::list_my_guilds(&pool, auth_user.id, limit, offset)
        .await
        .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::with_meta_message(
        "Guilds fetched successfully",
        guilds,
        total,
        limit,
        offset,
    )))
}

/// Public-but-personalized: membership fields appear only when signed in.
pub async fn get_guild(
    State(pool): State<DbPool>,
    viewer: OptionalAuthUser,
    VPath(path): VPath<GuildSlugPath>,
) -> Result<Json<ApiResponse<GuildResponse>>, AppError> {
    let guild = services::guild::get_guild_by_slug(&pool, &path.slug, viewer_id(&viewer))
        .await
        .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Guild fetched successfully",
        guild,
    )))
}

pub async fn update_guild(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildSlugPath>,
    VJson(req): VJson<UpdateGuildRequest>,
) -> Result<Json<ApiResponse<GuildResponse>>, AppError> {
    let guild = services::guild::update_guild(
        &pool,
        &path.slug,
        auth_user.id,
        UpdateGuildInput {
            name: req.name,
            description: req.description,
            avatar_url: req.avatar_url,
            is_public: req.is_public,
        },
    )
    .await
    .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Guild updated successfully",
        guild,
    )))
}

pub async fn delete_guild(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildSlugPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::guild::delete_guild(&pool, &path.slug, auth_user.id)
        .await
        .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Guild deleted successfully",
        serde_json::Value::Null,
    )))
}

pub async fn join_guild(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildSlugPath>,
) -> Result<(StatusCode, Json<ApiResponse<GuildMemberResponse>>), AppError> {
    let member = services::guild::join_guild(&pool, &path.slug, auth_user.id)
        .await
        .map_err(map_guild_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Joined guild successfully",
            member,
        )),
    ))
}

pub async fn leave_guild(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildSlugPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::guild::leave_guild(&pool, &path.slug, auth_user.id)
        .await
        .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Left guild successfully",
        serde_json::Value::Null,
    )))
}

pub async fn get_members(
    State(pool): State<DbPool>,
    viewer: OptionalAuthUser,
    VPath(path): VPath<GuildSlugPath>,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<GuildMemberResponse>>>, AppError> {
    let (limit, offset) = query.resolve(50);
    let (members, total) =
        services::guild::list_members(&pool, &path.slug, viewer_id(&viewer), limit, offset)
            .await
            .map_err(map_guild_error)?;
    Ok(Json(ApiResponse::with_meta_message(
        "Guild members fetched successfully",
        members,
        total,
        limit,
        offset,
    )))
}

pub async fn list_channels(
    State(pool): State<DbPool>,
    viewer: OptionalAuthUser,
    VPath(path): VPath<GuildSlugPath>,
) -> Result<Json<ApiResponse<Vec<GuildChannelResponse>>>, AppError> {
    let channels = services::guild_channel::list_channels(&pool, &path.slug, viewer_id(&viewer))
        .await
        .map_err(map_guild_channel_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Channels fetched successfully",
        channels,
    )))
}

pub async fn create_channel(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildSlugPath>,
    VJson(req): VJson<CreateGuildChannelRequest>,
) -> Result<(StatusCode, Json<ApiResponse<GuildChannelResponse>>), AppError> {
    let channel = services::guild_channel::create_channel(
        &pool,
        &path.slug,
        auth_user.id,
        &req.name,
        req.topic,
        req.position,
    )
    .await
    .map_err(map_guild_channel_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Channel created successfully",
            channel,
        )),
    ))
}

pub async fn update_channel(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildChannelPath>,
    VJson(req): VJson<UpdateGuildChannelRequest>,
) -> Result<Json<ApiResponse<GuildChannelResponse>>, AppError> {
    let channel = services::guild_channel::update_channel(
        &pool,
        &path.slug,
        &path.channel_id,
        auth_user.id,
        UpdateChannelInput {
            name: req.name,
            topic: req.topic,
            position: req.position,
        },
    )
    .await
    .map_err(map_guild_channel_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Channel updated successfully",
        channel,
    )))
}

pub async fn delete_channel(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildChannelPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::guild_channel::delete_channel(&pool, &path.slug, &path.channel_id, auth_user.id)
        .await
        .map_err(map_guild_channel_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Channel deleted successfully",
        serde_json::Value::Null,
    )))
}

/// Channel history newest first, paginated backwards with the
/// `?before=<message id>` cursor rather than offset.
pub async fn list_messages(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildChannelPath>,
    VQuery(query): VQuery<GuildMessagesQuery>,
) -> Result<Json<GuildMessagesResponse>, AppError> {
    let (messages, meta) = services::guild_channel::list_messages(
        &pool,
        &path.slug,
        &path.channel_id,
        auth_user.id,
        query.before.as_deref(),
        query.resolve_limit(),
    )
    .await
    .map_err(map_guild_channel_error)?;
    Ok(Json(GuildMessagesResponse {
        success: true,
        message: "Messages fetched successfully".to_string(),
        data: messages,
        meta,
    }))
}

pub async fn send_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildChannelPath>,
    VJson(req): VJson<CreateGuildMessageRequest>,
) -> Result<(StatusCode, Json<ApiResponse<GuildMessageResponse>>), AppError> {
    let message = services::guild_channel::send_message(
        &pool,
        &path.slug,
        &path.channel_id,
        auth_user.id,
        &req.content,
        req.reply_to_id.as_deref(),
    )
    .await
    .map_err(map_guild_channel_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Message sent successfully",
            message,
        )),
    ))
}

pub async fn edit_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildMessagePath>,
    VJson(req): VJson<UpdateGuildMessageRequest>,
) -> Result<Json<ApiResponse<GuildMessageResponse>>, AppError> {
    let message = services::guild_channel::edit_message(
        &pool,
        &path.slug,
        &path.channel_id,
        &path.message_id,
        auth_user.id,
        &req.content,
    )
    .await
    .map_err(map_guild_channel_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Message updated successfully",
        message,
    )))
}

pub async fn delete_message(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildMessagePath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::guild_channel::delete_message(
        &pool,
        &path.slug,
        &path.channel_id,
        &path.message_id,
        auth_user.id,
    )
    .await
    .map_err(map_guild_channel_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Message deleted successfully",
        serde_json::Value::Null,
    )))
}

enum StreamStep {
    Event(std::sync::Arc<str>),
    Heartbeat,
    Closed,
}

/// Holds a Server-Sent Events stream open and forwards every realtime event of
/// the guild (all channels) to a member. Each event is one `data:` line
/// holding a `GuildEvent`. The stream ends when the client disconnects, the
/// member leaves the guild, or the subscriber falls too far behind; clients
/// should reconnect and refetch recent history to fill any gap.
pub async fn stream_events(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(path): VPath<GuildSlugPath>,
) -> Result<Response, AppError> {
    let user_id = auth_user.id;
    let (guild_id, mut sub) = services::guild_channel::subscribe(&pool, &path.slug, user_id)
        .await
        .map_err(map_guild_channel_error)?;

    let stream = async_stream::stream! {
        yield Ok::<_, Infallible>(Event::default().comment("connected"));

        let start = tokio::time::Instant::now() + GUILD_STREAM_HEARTBEAT;
        let mut heartbeat = tokio::time::interval_at(start, GUILD_STREAM_HEARTBEAT);
        loop {
            let step = tokio::select! {
                payload = sub.recv() => match payload {
                    Some(payload) => StreamStep::Event(payload),
                    None => StreamStep::Closed,
                },
                _ = heartbeat.tick() => StreamStep::Heartbeat,
            };
            match step {
                StreamStep::Event(payload) => yield Ok(Event::default().data(&*payload)),
                StreamStep::Closed => break,
                StreamStep::Heartbeat => {
                    match services::guild_channel::is_member(&pool, guild_id, user_id).await {
                        Ok(false) => break,
                        Ok(true) => {}
                        Err(err) => tracing::warn!(
                            error = ?err,
                            %guild_id,
                            "guild chat: membership re-check failed"
                        ),
                    }
                    yield Ok(Event::default().comment("ping"));
                }
            }
        }
    };

    Ok((
        // Stop nginx-style proxies from buffering the stream.
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(stream),
    )
        .into_response())
}

/// Guild routes. Static segments like `/api/guilds/me` take precedence over
/// `/{slug}`, and `services::guild` refuses to create a guild with a reserved
/// slug so none can be shadowed.
pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/guilds", get(list_guilds).post(create_guild))
        .route("/api/guilds/me", get(get_my_guilds))
        .route(
            "/api/guilds/{slug}",
            get(get_guild).patch(update_guild).delete(delete_guild),
        )
        .route("/api/guilds/{slug}/members", get(get_members))
        .route("/api/guilds/{slug}/join", post(join_guild))
        .route("/api/guilds/{slug}/leave", delete(leave_guild))
        .route(
            "/api/guilds/{slug}/channels",
            get(list_channels).post(create_channel),
        )
        .route(
            "/api/guilds/{slug}/channels/{channelId}",
            patch(update_channel).delete(delete_channel),
        )
        .route(
            "/api/guilds/{slug}/channels/{channelId}/messages",
            get(list_messages).post(send_message),
        )
        .route(
            "/api/guilds/{slug}/channels/{channelId}/messages/{messageId}",
            patch(edit_message).delete(delete_message),
        )
}

/// The realtime stream is long-lived, so it is registered outside the request
/// timeout layer.
pub fn stream_routes() -> Router<DbPool> {
    Router::new().route("/api/guilds/{slug}/events", get(stream_events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_member_maps_differently_per_endpoint_group() {
        assert!(matches!(
            map_guild_error(GuildError::NotMember),
            AppError::BadRequest(msg) if msg == "not a member of this guild"
        ));
        assert!(matches!(
            map_guild_channel_error(GuildError::NotMember),
            AppError::Forbidden(_)
        ));
    }

    #[test]
    fn test_guild_error_mapping() {
        assert!(matches!(
            map_guild_error(GuildError::SlugExists),
            AppError::Conflict(_)
        ));
        assert!(matches!(
            map_guild_error(GuildError::OwnerCannotLeave),
            AppError::BadRequest(_)
        ));
        assert!(matches!(
            map_guild_channel_error(GuildError::InvalidMessageCursor),
            AppError::BadRequest(msg) if msg == "invalid message cursor"
        ));
        assert!(matches!(
            map_guild_channel_error(GuildError::ChannelNameExists),
            AppError::Conflict(_)
        ));
    }
}
