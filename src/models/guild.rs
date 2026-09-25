use super::user::User;
use crate::entities::{guild_channel_messages, guild_channels, guild_members, guilds};
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Guild member roles. The owner row is created together with the guild and
/// is the only row that may hold [`GUILD_ROLE_OWNER`].
pub const GUILD_ROLE_OWNER: &str = "owner";
pub const GUILD_ROLE_ADMIN: &str = "admin";
pub const GUILD_ROLE_MEMBER: &str = "member";

/// The channel every guild is created with.
pub const GUILD_DEFAULT_CHANNEL_NAME: &str = "general";

/// Caps the quoted parent text: a reply shows a one-line preview, not the full
/// (up to 4000 characters) parent.
pub const GUILD_MESSAGE_REPLY_PREVIEW_LEN: usize = 200;

// Guild realtime event types.
pub const GUILD_EVENT_CHANNEL_CREATED: &str = "channel.created";
pub const GUILD_EVENT_CHANNEL_UPDATED: &str = "channel.updated";
pub const GUILD_EVENT_CHANNEL_DELETED: &str = "channel.deleted";
pub const GUILD_EVENT_MESSAGE_CREATED: &str = "message.created";
pub const GUILD_EVENT_MESSAGE_UPDATED: &str = "message.updated";
pub const GUILD_EVENT_MESSAGE_DELETED: &str = "message.deleted";

pub fn is_guild_manager(role: &str) -> bool {
    role == GUILD_ROLE_OWNER || role == GUILD_ROLE_ADMIN
}

fn to_utc(value: DateTime<FixedOffset>) -> DateTime<Utc> {
    value.with_timezone(&Utc)
}

#[derive(Serialize, Clone)]
pub struct GuildResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub is_public: bool,
    pub member_count: i64,
    pub owner_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<User>,
    /// `is_member` and `my_role` are only set for authenticated callers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_member: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_role: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl GuildResponse {
    pub fn from_entity(guild: guilds::Model, owner: Option<User>) -> Self {
        Self {
            id: guild.id,
            name: guild.name,
            slug: guild.slug,
            description: guild.description,
            avatar_url: guild.avatar_url,
            is_public: guild.is_public,
            member_count: guild.member_count,
            owner_id: guild.owner_id,
            owner,
            is_member: None,
            my_role: None,
            created_at: guild.created_at.map(to_utc),
            updated_at: guild.updated_at.map(to_utc),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct GuildMemberResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    pub created_at: Option<DateTime<Utc>>,
}

impl GuildMemberResponse {
    pub fn from_entity(member: guild_members::Model, user: Option<User>) -> Self {
        Self {
            id: member.id,
            guild_id: member.guild_id,
            user_id: member.user_id,
            role: member.role,
            user,
            created_at: member.created_at.map(to_utc),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct GuildChannelResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub topic: Option<String>,
    pub position: i32,
    pub created_by: Option<Uuid>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<guild_channels::Model> for GuildChannelResponse {
    fn from(channel: guild_channels::Model) -> Self {
        Self {
            id: channel.id,
            guild_id: channel.guild_id,
            name: channel.name,
            topic: channel.topic,
            position: channel.position,
            created_by: channel.created_by,
            created_at: Some(to_utc(channel.created_at)),
            updated_at: Some(to_utc(channel.updated_at)),
        }
    }
}

/// The parent message quoted above a reply. `content` is truncated to
/// [`GUILD_MESSAGE_REPLY_PREVIEW_LEN`] characters.
#[derive(Serialize, Clone)]
pub struct GuildMessageReply {
    pub id: Uuid,
    pub author_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<User>,
    pub content: String,
}

impl GuildMessageReply {
    pub fn from_entity(parent: guild_channel_messages::Model, author: Option<User>) -> Self {
        Self {
            id: parent.id,
            author_id: parent.author_id,
            author,
            content: truncate_chars(&parent.content, GUILD_MESSAGE_REPLY_PREVIEW_LEN),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct GuildMessageResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub channel_id: Uuid,
    pub author_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<User>,
    pub content: String,
    pub reply_to_id: Option<Uuid>,
    /// A quote of the parent message; absent once the parent is deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<GuildMessageReply>,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

impl GuildMessageResponse {
    pub fn from_entity(
        message: guild_channel_messages::Model,
        guild_id: Uuid,
        author: Option<User>,
        reply_to: Option<GuildMessageReply>,
    ) -> Self {
        Self {
            id: message.id,
            guild_id,
            channel_id: message.channel_id,
            author_id: message.author_id,
            author,
            content: message.content,
            reply_to_id: message.reply_to_id,
            reply_to,
            edited_at: message.edited_at.map(to_utc),
            created_at: Some(to_utc(message.created_at)),
        }
    }
}

/// Paginates channel history backwards. Pass `next_before` as `?before=` to
/// load the next (older) page.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct GuildMessageCursorMeta {
    pub limit: i64,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_before: Option<Uuid>,
}

/// The `ApiResponse` envelope with a cursor `meta` instead of offset
/// pagination, mirroring echobackend's `SuccessWithMeta(..., cursorMeta)`.
#[derive(Serialize)]
pub struct GuildMessagesResponse {
    pub success: bool,
    pub message: String,
    pub data: Vec<GuildMessageResponse>,
    pub meta: GuildMessageCursorMeta,
}

/// One realtime event pushed to guild subscribers over SSE.
#[derive(Serialize, Clone)]
pub struct GuildEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub guild_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<Uuid>,
    pub data: serde_json::Value,
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => s[..idx].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello", 5), "hello");
        assert_eq!(truncate_chars("hello", 3), "hel");
        assert_eq!(truncate_chars("héllo wörld", 7), "héllo w");
    }

    #[test]
    fn test_guild_event_serialization() {
        let event = GuildEvent {
            event_type: GUILD_EVENT_CHANNEL_DELETED,
            guild_id: Uuid::nil(),
            channel_id: None,
            data: serde_json::json!({ "id": "x" }),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["type"], "channel.deleted");
        assert!(value.get("channel_id").is_none());
    }

    #[test]
    fn test_is_guild_manager() {
        assert!(is_guild_manager(GUILD_ROLE_OWNER));
        assert!(is_guild_manager(GUILD_ROLE_ADMIN));
        assert!(!is_guild_manager(GUILD_ROLE_MEMBER));
        assert!(!is_guild_manager(""));
    }
}
