//! Discord-style text channels of a guild (echobackend's
//! `GuildChannelService`).
//!
//! Access rules:
//! - the channel list is visible to whoever can see the guild;
//! - reading, sending and streaming messages require membership;
//! - creating, renaming and deleting channels requires owner or admin;
//! - a message may be edited by its author only, and deleted by its author or
//!   a guild owner/admin.
//!
//! A private guild is reported as not found to non-members, as in
//! `services::guild`. Every request starts with a single [`resolve_access`]
//! query that yields the guild, the caller's role and the channel together.

use crate::entities::{guild_channel_messages, guild_channels, guild_members};
use crate::models::guild::{
    GUILD_EVENT_CHANNEL_CREATED, GUILD_EVENT_CHANNEL_DELETED, GUILD_EVENT_CHANNEL_UPDATED,
    GUILD_EVENT_MESSAGE_CREATED, GUILD_EVENT_MESSAGE_DELETED, GUILD_EVENT_MESSAGE_UPDATED,
    GuildChannelResponse, GuildEvent, GuildMessageCursorMeta, GuildMessageReply,
    GuildMessageResponse, is_guild_manager,
};
use crate::realtime::{self, Subscription};
use crate::services::guild::{GuildError, is_unique_violation};
use crate::services::user_hydration::load_user_brief_map;
use crate::slug;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    FromQueryResult, IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    Statement,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Mirrors the varchar(100) column on `guild_channels.name`.
const GUILD_CHANNEL_NAME_MAX_LEN: usize = 100;

/// The realtime topic carrying every event of one guild.
pub fn guild_event_topic(guild_id: Uuid) -> String {
    format!("guild:{guild_id}")
}

/// Everything an access check needs, read in one query.
#[derive(Debug, FromQueryResult)]
pub struct GuildChannelAccess {
    pub guild_id: Uuid,
    pub is_public: bool,
    /// The caller's guild role, empty when not a member.
    pub role: String,
    /// Set only when a channel was requested and it belongs to the guild.
    pub channel_id: Option<Uuid>,
}

/// Loads the guild by slug together with the caller's role and, when
/// `channel_id` is given, whether that channel is in the guild.
async fn resolve_access(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Option<Uuid>,
    channel_id: Option<Uuid>,
) -> Result<GuildChannelAccess, GuildError> {
    // Absent ids bind as NULL, and "= NULL" simply never matches.
    GuildChannelAccess::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"SELECT g.id AS guild_id, g.is_public, COALESCE(gm.role, '') AS role, c.id AS channel_id
           FROM guilds g
           LEFT JOIN guild_members gm ON gm.guild_id = g.id AND gm.user_id = $1
           LEFT JOIN guild_channels c ON c.guild_id = g.id AND c.id = $2
           WHERE g.slug = $3"#,
        [user_id.into(), channel_id.into(), guild_slug.into()],
    ))
    .one(db)
    .await?
    .ok_or(GuildError::GuildNotFound)
}

/// Resolves access and hides private guilds from non-members.
async fn resolve_guild(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Option<Uuid>,
    channel_id: Option<Uuid>,
) -> Result<GuildChannelAccess, GuildError> {
    let access = resolve_access(db, guild_slug, user_id, channel_id).await?;
    if !access.is_public && access.role.is_empty() {
        return Err(GuildError::GuildNotFound);
    }
    Ok(access)
}

/// Checks for an owner or admin. The caller checks `access.channel_id` itself
/// when it passed a channel.
async fn require_manager(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
    channel_id: Option<Uuid>,
) -> Result<GuildChannelAccess, GuildError> {
    let access = resolve_guild(db, guild_slug, Some(user_id), channel_id).await?;
    if !is_guild_manager(&access.role) {
        return Err(GuildError::NotOwned);
    }
    Ok(access)
}

/// Checks that the caller is a member of the guild and that the channel
/// belongs to it. Returns the access and the parsed channel id.
async fn resolve_member_channel(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    user_id: Uuid,
) -> Result<(GuildChannelAccess, Uuid), GuildError> {
    // A malformed id is looked up as "no channel", so the guild checks still
    // run first and a hidden guild keeps reading as missing.
    let channel_id = Uuid::parse_str(channel_id).ok();
    let access = resolve_guild(db, guild_slug, Some(user_id), channel_id).await?;
    if access.role.is_empty() {
        return Err(GuildError::NotMember);
    }
    let channel_id = access.channel_id.ok_or(GuildError::ChannelNotFound)?;
    Ok((access, channel_id))
}

/// Pushes an event to live subscribers. The change is already committed, so a
/// delivery failure is logged rather than returned: clients recover by
/// refetching history when they reconnect.
fn publish(
    guild_id: Uuid,
    channel_id: Option<Uuid>,
    event_type: &'static str,
    data: impl Serialize,
) {
    let data = match serde_json::to_value(data) {
        Ok(data) => data,
        Err(err) => {
            tracing::warn!(error = %err, event_type, %guild_id, "guild chat: failed to encode event");
            return;
        }
    };
    let event = GuildEvent {
        event_type,
        guild_id,
        channel_id,
        data,
    };
    if let Err(err) = realtime::hub().publish(&guild_event_topic(guild_id), &event) {
        tracing::warn!(error = %err, event_type, %guild_id, "guild chat: failed to publish event");
    }
}

pub async fn list_channels(
    db: &DatabaseConnection,
    guild_slug: &str,
    viewer_id: Option<Uuid>,
) -> Result<Vec<GuildChannelResponse>, GuildError> {
    let access = resolve_guild(db, guild_slug, viewer_id, None).await?;
    let channels = guild_channels::Entity::find()
        .filter(guild_channels::Column::GuildId.eq(access.guild_id))
        .order_by_asc(guild_channels::Column::Position)
        .order_by_asc(guild_channels::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(channels.into_iter().map(Into::into).collect())
}

pub struct UpdateChannelInput {
    pub name: Option<String>,
    pub topic: Option<String>,
    pub position: Option<i32>,
}

fn channel_name(raw: &str) -> Result<String, GuildError> {
    let name = slug::make(raw, GUILD_CHANNEL_NAME_MAX_LEN);
    if name.is_empty() {
        return Err(GuildError::ChannelNameInvalid);
    }
    Ok(name)
}

fn map_channel_write_error(err: DbErr) -> GuildError {
    // Unique (guild_id, name).
    if is_unique_violation(&err) {
        GuildError::ChannelNameExists
    } else {
        err.into()
    }
}

pub async fn create_channel(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
    name: &str,
    topic: Option<String>,
    position: Option<i32>,
) -> Result<GuildChannelResponse, GuildError> {
    let access = require_manager(db, guild_slug, user_id, None).await?;
    let name = channel_name(name)?;

    let now = Utc::now();
    let channel = guild_channels::ActiveModel {
        id: Set(Uuid::now_v7()),
        guild_id: Set(access.guild_id),
        name: Set(name),
        topic: Set(topic),
        position: Set(position.unwrap_or(0)),
        created_by: Set(Some(user_id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(db)
    .await
    .map_err(map_channel_write_error)?;

    let resp = GuildChannelResponse::from(channel);
    publish(
        access.guild_id,
        Some(resp.id),
        GUILD_EVENT_CHANNEL_CREATED,
        &resp,
    );
    Ok(resp)
}

pub async fn update_channel(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    user_id: Uuid,
    input: UpdateChannelInput,
) -> Result<GuildChannelResponse, GuildError> {
    let access = require_manager(db, guild_slug, user_id, Uuid::parse_str(channel_id).ok()).await?;
    let channel_id = access.channel_id.ok_or(GuildError::ChannelNotFound)?;
    let name = input.name.as_deref().map(channel_name).transpose()?;

    let channel = guild_channels::Entity::find_by_id(channel_id)
        .filter(guild_channels::Column::GuildId.eq(access.guild_id))
        .one(db)
        .await?
        .ok_or(GuildError::ChannelNotFound)?;

    let changed = name.is_some() || input.topic.is_some() || input.position.is_some();
    if !changed {
        return Ok(channel.into());
    }

    let mut active = channel.into_active_model();
    if let Some(name) = name {
        active.name = Set(name);
    }
    if let Some(topic) = input.topic {
        active.topic = Set(Some(topic));
    }
    if let Some(position) = input.position {
        active.position = Set(position);
    }
    active.updated_at = Set(Utc::now().into());
    let updated = active.update(db).await.map_err(map_channel_write_error)?;

    let resp = GuildChannelResponse::from(updated);
    publish(
        access.guild_id,
        Some(channel_id),
        GUILD_EVENT_CHANNEL_UPDATED,
        &resp,
    );
    Ok(resp)
}

pub async fn delete_channel(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    user_id: Uuid,
) -> Result<(), GuildError> {
    let access = require_manager(db, guild_slug, user_id, Uuid::parse_str(channel_id).ok()).await?;
    let channel_id = access.channel_id.ok_or(GuildError::ChannelNotFound)?;

    let result = guild_channels::Entity::delete_many()
        .filter(guild_channels::Column::Id.eq(channel_id))
        .filter(guild_channels::Column::GuildId.eq(access.guild_id))
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(GuildError::ChannelNotFound);
    }
    publish(
        access.guild_id,
        Some(channel_id),
        GUILD_EVENT_CHANNEL_DELETED,
        serde_json::json!({ "id": channel_id }),
    );
    Ok(())
}

/// Attaches author briefs and reply quotes: one query for the parents, one
/// for every author on the page.
async fn message_responses(
    db: &DatabaseConnection,
    guild_id: Uuid,
    messages: Vec<guild_channel_messages::Model>,
) -> Result<Vec<GuildMessageResponse>, DbErr> {
    let reply_ids: HashSet<Uuid> = messages.iter().filter_map(|m| m.reply_to_id).collect();
    let parents: HashMap<Uuid, guild_channel_messages::Model> = if reply_ids.is_empty() {
        HashMap::new()
    } else {
        guild_channel_messages::Entity::find()
            .filter(guild_channel_messages::Column::Id.is_in(reply_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|parent| (parent.id, parent))
            .collect()
    };

    let authors = load_user_brief_map(
        db,
        messages
            .iter()
            .map(|m| m.author_id)
            .chain(parents.values().map(|p| p.author_id)),
    )
    .await?;

    Ok(messages
        .into_iter()
        .map(|message| {
            let reply_to = message
                .reply_to_id
                .and_then(|id| parents.get(&id))
                .map(|parent| {
                    GuildMessageReply::from_entity(
                        parent.clone(),
                        authors.get(&parent.author_id).cloned(),
                    )
                });
            let author = authors.get(&message.author_id).cloned();
            GuildMessageResponse::from_entity(message, guild_id, author, reply_to)
        })
        .collect())
}

async fn message_response(
    db: &DatabaseConnection,
    guild_id: Uuid,
    message: guild_channel_messages::Model,
) -> Result<GuildMessageResponse, DbErr> {
    let mut out = message_responses(db, guild_id, vec![message]).await?;
    Ok(out.remove(0))
}

async fn find_message(
    db: &DatabaseConnection,
    channel_id: Uuid,
    message_id: &str,
) -> Result<guild_channel_messages::Model, GuildError> {
    let message_id = Uuid::parse_str(message_id).map_err(|_| GuildError::MessageNotFound)?;
    guild_channel_messages::Entity::find_by_id(message_id)
        .filter(guild_channel_messages::Column::ChannelId.eq(channel_id))
        .one(db)
        .await?
        .ok_or(GuildError::MessageNotFound)
}

/// Returns channel history newest first, paginated backwards with the
/// `before` message-id cursor rather than offset, so that messages arriving
/// meanwhile do not shift the pages.
pub async fn list_messages(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    user_id: Uuid,
    before: Option<&str>,
    limit: i64,
) -> Result<(Vec<GuildMessageResponse>, GuildMessageCursorMeta), GuildError> {
    let before = match before.filter(|b| !b.is_empty()) {
        Some(raw) => Some(Uuid::parse_str(raw).map_err(|_| GuildError::InvalidMessageCursor)?),
        None => None,
    };
    let (access, channel_id) = resolve_member_channel(db, guild_slug, channel_id, user_id).await?;

    let mut query = guild_channel_messages::Entity::find()
        .filter(guild_channel_messages::Column::ChannelId.eq(channel_id));
    if let Some(before) = before {
        // uuidv7 ids sort in insert order, so the id is the cursor.
        query = query.filter(guild_channel_messages::Column::Id.lt(before));
    }
    // One extra row tells whether an older page exists without a COUNT.
    let mut messages = query
        .order_by_desc(guild_channel_messages::Column::Id)
        .limit((limit.max(0) + 1) as u64)
        .all(db)
        .await?;

    let mut meta = GuildMessageCursorMeta {
        limit,
        has_more: false,
        next_before: None,
    };
    if messages.len() as i64 > limit {
        messages.truncate(limit.max(0) as usize);
        meta.has_more = true;
        meta.next_before = messages.last().map(|m| m.id);
    }

    Ok((
        message_responses(db, access.guild_id, messages).await?,
        meta,
    ))
}

pub async fn send_message(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    user_id: Uuid,
    content: &str,
    reply_to_id: Option<&str>,
) -> Result<GuildMessageResponse, GuildError> {
    let content = content.trim();
    if content.is_empty() {
        return Err(GuildError::MessageEmpty);
    }
    let (access, channel_id) = resolve_member_channel(db, guild_slug, channel_id, user_id).await?;

    let reply_to_id = match reply_to_id.filter(|id| !id.is_empty()) {
        Some(raw) => Some(
            find_message(db, channel_id, raw)
                .await
                .map_err(|err| match err {
                    GuildError::MessageNotFound => GuildError::MessageReplyNotFound,
                    other => other,
                })?
                .id,
        ),
        None => None,
    };

    let message = guild_channel_messages::ActiveModel {
        id: Set(Uuid::now_v7()),
        channel_id: Set(channel_id),
        author_id: Set(user_id),
        content: Set(content.to_string()),
        reply_to_id: Set(reply_to_id),
        edited_at: Set(None),
        created_at: Set(Utc::now().into()),
    }
    .insert(db)
    .await?;

    let resp = message_response(db, access.guild_id, message).await?;
    publish(
        access.guild_id,
        Some(channel_id),
        GUILD_EVENT_MESSAGE_CREATED,
        &resp,
    );
    Ok(resp)
}

pub async fn edit_message(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    message_id: &str,
    user_id: Uuid,
    content: &str,
) -> Result<GuildMessageResponse, GuildError> {
    let content = content.trim();
    if content.is_empty() {
        return Err(GuildError::MessageEmpty);
    }
    let (access, channel_id) = resolve_member_channel(db, guild_slug, channel_id, user_id).await?;
    let message = find_message(db, channel_id, message_id).await?;
    // Moderators may delete other people's messages but never put words in
    // their mouth.
    if message.author_id != user_id {
        return Err(GuildError::MessageNotOwned);
    }

    let mut active = message.into_active_model();
    active.content = Set(content.to_string());
    active.edited_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;

    let resp = message_response(db, access.guild_id, updated).await?;
    publish(
        access.guild_id,
        Some(channel_id),
        GUILD_EVENT_MESSAGE_UPDATED,
        &resp,
    );
    Ok(resp)
}

pub async fn delete_message(
    db: &DatabaseConnection,
    guild_slug: &str,
    channel_id: &str,
    message_id: &str,
    user_id: Uuid,
) -> Result<(), GuildError> {
    let (access, channel_id) = resolve_member_channel(db, guild_slug, channel_id, user_id).await?;
    let message = find_message(db, channel_id, message_id).await?;
    if message.author_id != user_id && !is_guild_manager(&access.role) {
        return Err(GuildError::MessageNotOwned);
    }

    let result = guild_channel_messages::Entity::delete_by_id(message.id)
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(GuildError::MessageNotFound);
    }
    publish(
        access.guild_id,
        Some(channel_id),
        GUILD_EVENT_MESSAGE_DELETED,
        serde_json::json!({ "id": message.id, "channel_id": channel_id }),
    );
    Ok(())
}

/// Opens the realtime event stream of a guild for a member. The returned guild
/// id lets the caller re-check membership with [`is_member`] while the stream
/// stays open.
pub async fn subscribe(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
) -> Result<(Uuid, Subscription), GuildError> {
    let access = resolve_guild(db, guild_slug, Some(user_id), None).await?;
    if access.role.is_empty() {
        return Err(GuildError::NotMember);
    }
    let sub = realtime::hub().subscribe(&guild_event_topic(access.guild_id));
    Ok((access.guild_id, sub))
}

/// A cheap existence check for re-validating open streams.
pub async fn is_member(
    db: &DatabaseConnection,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, DbErr> {
    let count = guild_members::Entity::find()
        .filter(guild_members::Column::GuildId.eq(guild_id))
        .filter(guild_members::Column::UserId.eq(user_id))
        .count(db)
        .await?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guild_event_topic() {
        assert_eq!(
            guild_event_topic(Uuid::nil()),
            "guild:00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn test_channel_name_normalised() {
        assert_eq!(channel_name("Off Topic!").unwrap(), "off-topic");
        assert!(matches!(
            channel_name("!!!"),
            Err(GuildError::ChannelNameInvalid)
        ));
    }
}
