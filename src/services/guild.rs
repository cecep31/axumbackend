use crate::entities::{guild_channels, guild_members, guilds};
use crate::models::guild::{
    GUILD_DEFAULT_CHANNEL_NAME, GUILD_ROLE_MEMBER, GUILD_ROLE_OWNER, GuildMemberResponse,
    GuildResponse, is_guild_manager,
};
use crate::services::user_hydration::load_user_brief_map;
use crate::slug;
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    JoinType, NotSet, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set,
    SqlErr, TransactionTrait,
};
use uuid::Uuid;

/// Mirrors the varchar(100) column on `guilds.slug`.
const GUILD_SLUG_MAX_LEN: usize = 100;

/// Taken by static routes under `/api/guilds`. A guild holding one of these
/// would be created but unreachable by URL, so creation is rejected instead.
/// Keep in sync with `handlers/guild.rs`.
const RESERVED_GUILD_SLUGS: &[&str] = &["me"];

/// Domain errors for guilds and their channels (echobackend's guild
/// `apperrors`). `Display` gives the error text echobackend returns for the
/// `400` cases.
#[derive(Debug)]
pub enum GuildError {
    Db(DbErr),
    GuildNotFound,
    SlugExists,
    SlugInvalid,
    SlugReserved,
    NotOwned,
    AlreadyMember,
    NotMember,
    OwnerCannotLeave,
    ChannelNotFound,
    ChannelNameExists,
    ChannelNameInvalid,
    MessageNotFound,
    MessageNotOwned,
    MessageEmpty,
    MessageReplyNotFound,
    InvalidMessageCursor,
}

impl std::fmt::Display for GuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            GuildError::Db(err) => return write!(f, "{err}"),
            GuildError::GuildNotFound => "guild not found",
            GuildError::SlugExists => "guild slug already taken",
            GuildError::SlugInvalid => "guild name must contain at least one letter or digit",
            GuildError::SlugReserved => "guild slug is reserved",
            GuildError::NotOwned => "not authorized to modify this guild",
            GuildError::AlreadyMember => "already a member of this guild",
            GuildError::NotMember => "not a member of this guild",
            GuildError::OwnerCannotLeave => {
                "guild owner cannot leave; transfer ownership or delete the guild"
            }
            GuildError::ChannelNotFound => "guild channel not found",
            GuildError::ChannelNameExists => "a channel with this name already exists in the guild",
            GuildError::ChannelNameInvalid => {
                "channel name must contain at least one letter or digit"
            }
            GuildError::MessageNotFound => "message not found",
            GuildError::MessageNotOwned => "not authorized to modify this message",
            GuildError::MessageEmpty => "message content cannot be empty",
            GuildError::MessageReplyNotFound => "replied-to message not found in this channel",
            GuildError::InvalidMessageCursor => "invalid message cursor",
        };
        f.write_str(message)
    }
}

impl From<DbErr> for GuildError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

pub(crate) fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

async fn find_guild_by_slug(
    db: &DatabaseConnection,
    guild_slug: &str,
) -> Result<guilds::Model, GuildError> {
    guilds::Entity::find()
        .filter(guilds::Column::Slug.eq(guild_slug))
        .one(db)
        .await?
        .ok_or(GuildError::GuildNotFound)
}

/// Returns `None` when `user_id` is absent or the user is not a member, so
/// callers can treat "anonymous" and "not joined" alike.
async fn lookup_membership(
    db: &DatabaseConnection,
    guild_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Option<guild_members::Model>, DbErr> {
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    guild_members::Entity::find()
        .filter(guild_members::Column::GuildId.eq(guild_id))
        .filter(guild_members::Column::UserId.eq(user_id))
        .one(db)
        .await
}

/// Builds guild responses with the owner brief attached.
async fn guild_responses(
    db: &DatabaseConnection,
    guilds: Vec<guilds::Model>,
) -> Result<Vec<GuildResponse>, DbErr> {
    let mut owners = load_user_brief_map(db, guilds.iter().map(|g| g.owner_id)).await?;
    Ok(guilds
        .into_iter()
        .map(|guild| {
            let owner = owners.remove(&guild.owner_id);
            GuildResponse::from_entity(guild, owner)
        })
        .collect())
}

async fn guild_response(
    db: &DatabaseConnection,
    guild: guilds::Model,
) -> Result<GuildResponse, DbErr> {
    let mut out = guild_responses(db, vec![guild]).await?;
    Ok(out.remove(0))
}

/// Fills the viewer-relative fields. They stay absent for anonymous callers
/// rather than being reported as false.
fn apply_membership(
    resp: &mut GuildResponse,
    member: Option<&guild_members::Model>,
    viewer_id: Option<Uuid>,
) {
    if viewer_id.is_none() {
        return;
    }
    resp.is_member = Some(member.is_some());
    resp.my_role = member.map(|m| m.role.clone());
}

async fn member_response(
    db: &DatabaseConnection,
    member: guild_members::Model,
) -> Result<GuildMemberResponse, DbErr> {
    let mut users = load_user_brief_map(db, [member.user_id]).await?;
    let user = users.remove(&member.user_id);
    Ok(GuildMemberResponse::from_entity(member, user))
}

pub struct CreateGuildInput {
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub is_public: Option<bool>,
}

/// Inserts the guild, its owner membership and its `#general` channel in one
/// transaction.
pub async fn create_guild(
    db: &DatabaseConnection,
    owner_id: Uuid,
    input: CreateGuildInput,
) -> Result<GuildResponse, GuildError> {
    let desired = input
        .slug
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&input.name);
    let generated = slug::make(desired, GUILD_SLUG_MAX_LEN);
    if generated.is_empty() {
        return Err(GuildError::SlugInvalid);
    }
    if RESERVED_GUILD_SLUGS.contains(&generated.as_str()) {
        return Err(GuildError::SlugReserved);
    }

    let now = Utc::now();
    let guild_id = Uuid::now_v7();
    let txn = db.begin().await?;
    let inserted = guilds::ActiveModel {
        id: Set(guild_id),
        owner_id: Set(owner_id),
        name: Set(input.name),
        slug: Set(generated),
        description: Set(input.description),
        avatar_url: Set(input.avatar_url),
        is_public: Set(input.is_public.unwrap_or(true)),
        member_count: NotSet,
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
    }
    .insert(&txn)
    .await;
    if let Err(err) = inserted {
        // The only unique constraint reachable here is guilds.slug.
        return Err(if is_unique_violation(&err) {
            GuildError::SlugExists
        } else {
            err.into()
        });
    }

    // The owner is a member from the start, which is also what makes
    // member_count start at 1 via the guild_members trigger.
    guild_members::ActiveModel {
        id: Set(Uuid::now_v7()),
        guild_id: Set(guild_id),
        user_id: Set(owner_id),
        role: Set(GUILD_ROLE_OWNER.to_string()),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
    }
    .insert(&txn)
    .await?;

    guild_channels::ActiveModel {
        id: Set(Uuid::now_v7()),
        guild_id: Set(guild_id),
        name: Set(GUILD_DEFAULT_CHANNEL_NAME.to_string()),
        topic: Set(None),
        position: Set(0),
        created_by: Set(Some(owner_id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

    // Re-read for the trigger-maintained member_count.
    let created = guilds::Entity::find_by_id(guild_id)
        .one(db)
        .await?
        .ok_or(GuildError::GuildNotFound)?;
    let mut resp = guild_response(db, created).await?;
    resp.is_member = Some(true);
    resp.my_role = Some(GUILD_ROLE_OWNER.to_string());
    Ok(resp)
}

pub async fn get_guild_by_slug(
    db: &DatabaseConnection,
    guild_slug: &str,
    viewer_id: Option<Uuid>,
) -> Result<GuildResponse, GuildError> {
    let guild = find_guild_by_slug(db, guild_slug).await?;
    let member = lookup_membership(db, guild.id, viewer_id).await?;

    // A private guild is invisible to non-members: report it as missing rather
    // than forbidden so that its existence does not leak.
    if !guild.is_public && member.is_none() {
        return Err(GuildError::GuildNotFound);
    }

    let mut resp = guild_response(db, guild).await?;
    apply_membership(&mut resp, member.as_ref(), viewer_id);
    Ok(resp)
}

/// Neutralises the LIKE wildcards in user input so that a search for "100%"
/// does not match everything. Postgres uses backslash as the default LIKE
/// escape character.
fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub async fn list_guilds(
    db: &DatabaseConnection,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<GuildResponse>, i64), GuildError> {
    let mut query = guilds::Entity::find().filter(guilds::Column::IsPublic.eq(true));
    if let Some(search) = search.map(str::trim).filter(|s| !s.is_empty()) {
        // ILIKE with a leading wildcard cannot use the btree index; acceptable
        // while the directory is small, revisit with pg_trgm when it is not.
        let pattern = format!("%{}%", escape_like_pattern(search));
        query = query.filter(Expr::cust_with_values(
            "(\"guilds\".\"name\" ILIKE $1 OR \"guilds\".\"slug\" ILIKE $2)",
            [pattern.clone(), pattern],
        ));
    }

    let total = query.clone().count(db).await? as i64;
    let models = query
        .order_by_desc(guilds::Column::MemberCount)
        .order_by_desc(guilds::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;
    Ok((guild_responses(db, models).await?, total))
}

/// Lists the guilds the caller belongs to, including private ones, most
/// recently joined first.
pub async fn list_my_guilds(
    db: &DatabaseConnection,
    user_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<GuildResponse>, i64), GuildError> {
    let query = guilds::Entity::find()
        .join(JoinType::InnerJoin, guilds::Relation::GuildMembers.def())
        .filter(guild_members::Column::UserId.eq(user_id));

    let total = query.clone().count(db).await? as i64;
    let models = query
        .order_by_desc(guild_members::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;
    Ok((guild_responses(db, models).await?, total))
}

pub struct UpdateGuildInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub is_public: Option<bool>,
}

pub async fn update_guild(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
    input: UpdateGuildInput,
) -> Result<GuildResponse, GuildError> {
    let guild = find_guild_by_slug(db, guild_slug).await?;
    let member = lookup_membership(db, guild.id, Some(user_id)).await?;
    if !member.as_ref().is_some_and(|m| is_guild_manager(&m.role)) {
        return Err(GuildError::NotOwned);
    }

    // The slug is deliberately immutable: it is the public URL of the guild.
    let changed = input.name.is_some()
        || input.description.is_some()
        || input.avatar_url.is_some()
        || input.is_public.is_some();
    let guild = if changed {
        let mut active = guild.into_active_model();
        if let Some(name) = input.name {
            active.name = Set(name);
        }
        if let Some(description) = input.description {
            active.description = Set(Some(description));
        }
        if let Some(avatar_url) = input.avatar_url {
            active.avatar_url = Set(Some(avatar_url));
        }
        if let Some(is_public) = input.is_public {
            active.is_public = Set(is_public);
        }
        active.updated_at = Set(Some(Utc::now().into()));
        active.update(db).await?
    } else {
        guild
    };

    let mut resp = guild_response(db, guild).await?;
    apply_membership(&mut resp, member.as_ref(), Some(user_id));
    Ok(resp)
}

pub async fn delete_guild(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
) -> Result<(), GuildError> {
    let guild = find_guild_by_slug(db, guild_slug).await?;
    if guild.owner_id != user_id {
        return Err(GuildError::NotOwned);
    }
    let result = guilds::Entity::delete_by_id(guild.id).exec(db).await?;
    if result.rows_affected == 0 {
        return Err(GuildError::GuildNotFound);
    }
    Ok(())
}

pub async fn join_guild(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
) -> Result<GuildMemberResponse, GuildError> {
    let guild = find_guild_by_slug(db, guild_slug).await?;
    // Private guilds have no open join path yet; hide them the same way the
    // detail endpoint does.
    if !guild.is_public {
        return Err(GuildError::GuildNotFound);
    }
    if lookup_membership(db, guild.id, Some(user_id))
        .await?
        .is_some()
    {
        return Err(GuildError::AlreadyMember);
    }

    let now = Utc::now();
    let member = guild_members::ActiveModel {
        id: Set(Uuid::now_v7()),
        guild_id: Set(guild.id),
        user_id: Set(user_id),
        role: Set(GUILD_ROLE_MEMBER.to_string()),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
    }
    .insert(db)
    .await
    .map_err(|err| {
        // Unique (guild_id, user_id): a concurrent join lost the race.
        if is_unique_violation(&err) {
            GuildError::AlreadyMember
        } else {
            err.into()
        }
    })?;
    Ok(member_response(db, member).await?)
}

pub async fn leave_guild(
    db: &DatabaseConnection,
    guild_slug: &str,
    user_id: Uuid,
) -> Result<(), GuildError> {
    let guild = find_guild_by_slug(db, guild_slug).await?;
    let member = lookup_membership(db, guild.id, Some(user_id))
        .await?
        .ok_or(GuildError::NotMember)?;
    // Letting the owner leave would strand the guild with nobody able to
    // administer it.
    if member.role == GUILD_ROLE_OWNER {
        return Err(GuildError::OwnerCannotLeave);
    }
    let result = guild_members::Entity::delete_by_id(member.id)
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(GuildError::NotMember);
    }
    Ok(())
}

pub async fn list_members(
    db: &DatabaseConnection,
    guild_slug: &str,
    viewer_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<GuildMemberResponse>, i64), GuildError> {
    let guild = find_guild_by_slug(db, guild_slug).await?;
    if !guild.is_public && lookup_membership(db, guild.id, viewer_id).await?.is_none() {
        return Err(GuildError::GuildNotFound);
    }

    let query = guild_members::Entity::find().filter(guild_members::Column::GuildId.eq(guild.id));
    let total = query.clone().count(db).await? as i64;
    let members = query
        .order_by_desc(guild_members::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let mut users = load_user_brief_map(db, members.iter().map(|m| m.user_id)).await?;
    let out = members
        .into_iter()
        .map(|member| {
            let user = users.remove(&member.user_id);
            GuildMemberResponse::from_entity(member, user)
        })
        .collect();
    Ok((out, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_like_pattern() {
        assert_eq!(escape_like_pattern("100%"), "100\\%");
        assert_eq!(escape_like_pattern("a_b"), "a\\_b");
        assert_eq!(escape_like_pattern("c:\\x"), "c:\\\\x");
        assert_eq!(escape_like_pattern("plain"), "plain");
    }

    #[test]
    fn test_reserved_slugs() {
        assert!(RESERVED_GUILD_SLUGS.contains(&slug::make("Me", GUILD_SLUG_MAX_LEN).as_str()));
    }

    #[test]
    fn test_apply_membership() {
        let guild = guilds::Model {
            id: Uuid::nil(),
            owner_id: Uuid::nil(),
            name: "g".into(),
            slug: "g".into(),
            description: None,
            avatar_url: None,
            is_public: true,
            member_count: 1,
            created_at: None,
            updated_at: None,
        };

        let mut anon = GuildResponse::from_entity(guild.clone(), None);
        apply_membership(&mut anon, None, None);
        assert_eq!(anon.is_member, None);
        assert_eq!(anon.my_role, None);

        let mut outsider = GuildResponse::from_entity(guild.clone(), None);
        apply_membership(&mut outsider, None, Some(Uuid::nil()));
        assert_eq!(outsider.is_member, Some(false));
        assert_eq!(outsider.my_role, None);

        let member = guild_members::Model {
            id: Uuid::nil(),
            guild_id: Uuid::nil(),
            user_id: Uuid::nil(),
            role: GUILD_ROLE_MEMBER.into(),
            created_at: None,
            updated_at: None,
        };
        let mut joined = GuildResponse::from_entity(guild, None);
        apply_membership(&mut joined, Some(&member), Some(Uuid::nil()));
        assert_eq!(joined.is_member, Some(true));
        assert_eq!(joined.my_role.as_deref(), Some(GUILD_ROLE_MEMBER));
    }
}
