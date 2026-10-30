use crate::entities::{posts, posts_to_tags, tags, users};
use crate::infra::cache;
use crate::models::post::{Post, SitemapPost};
use chrono::Utc;
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::sea_query::{Expr, OnConflict, SimpleExpr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    ExprTrait, FromQueryResult, JoinType, ModelTrait, Order, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, RelationTrait, Select, Set,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub enum SortDirection {
    Asc,
    Desc,
}

fn validate_order_field(order_by: Option<&str>) -> posts::Column {
    match order_by {
        Some("id") => posts::Column::Id,
        Some("title") => posts::Column::Title,
        Some("updated_at") => posts::Column::UpdatedAt,
        Some("view_count") => posts::Column::ViewCount,
        Some("like_count") => posts::Column::LikeCount,
        Some("bookmark_count") => posts::Column::BookmarkCount,
        _ => posts::Column::CreatedAt,
    }
}

fn get_order_dir(dir: Option<SortDirection>) -> Order {
    match dir {
        Some(SortDirection::Asc) => Order::Asc,
        _ => Order::Desc,
    }
}

async fn hydrate_post(
    post: &posts::Model,
    user: Option<users::Model>,
    tags: Vec<tags::Model>,
    truncate_body: bool,
) -> Result<Post, DbErr> {
    Ok(Post::from_entity(post.clone(), user, tags, truncate_body))
}

async fn hydrate_posts(
    db: &DatabaseConnection,
    posts: Vec<posts::Model>,
    truncate_body: bool,
) -> Result<Vec<Post>, DbErr> {
    if posts.is_empty() {
        return Ok(Vec::new());
    }

    let post_ids: Vec<uuid::Uuid> = posts.iter().map(|p| p.id).collect();

    let users_map: std::collections::HashMap<uuid::Uuid, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(posts.iter().map(|p| p.created_by)))
        .all(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();

    let tags_map: std::collections::HashMap<uuid::Uuid, Vec<tags::Model>> = {
        #[derive(sea_orm::FromQueryResult)]
        struct PostTagRow {
            post_id: uuid::Uuid,
            tag_id: i32,
            tag_name: String,
        }

        let params = sea_orm::sea_query::Value::from(post_ids.clone());
        let rows: Vec<PostTagRow> = PostTagRow::find_by_statement(
            sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Postgres,
                "SELECT ptt.post_id, t.id AS tag_id, t.name AS tag_name FROM posts_to_tags ptt INNER JOIN tags t ON ptt.tag_id = t.id WHERE ptt.post_id = ANY($1)",
                vec![params],
            ),
        )
        .all(db)
        .await?;

        let mut map: std::collections::HashMap<uuid::Uuid, Vec<tags::Model>> =
            std::collections::HashMap::new();
        for row in rows {
            let tag = tags::Model {
                id: row.tag_id,
                name: row.tag_name,
                created_at: None,
            };
            map.entry(row.post_id).or_default().push(tag);
        }
        map
    };

    let mut hydrated = Vec::with_capacity(posts.len());
    for post in &posts {
        let user = users_map.get(&post.created_by).cloned();
        let tags = tags_map.get(&post.id).cloned().unwrap_or_default();
        hydrated.push(hydrate_post(post, user, tags, truncate_body).await?);
    }

    Ok(hydrated)
}

/// echobackend's `activePostUserJoin`: every post read joins the author and
/// drops posts whose author is soft-deleted
/// (`JOIN users ON users.id = posts.created_by AND users.deleted_at IS NULL`).
pub fn active_posts() -> Select<posts::Entity> {
    posts::Entity::find()
        .join(JoinType::InnerJoin, posts::Relation::Users.def())
        .filter(users::Column::DeletedAt.is_null())
}

/// echobackend's `GetPostByID`: the post (drafts and hidden posts included)
/// unless it is missing or its author is soft-deleted.
pub async fn find_active_post(
    db: &impl ConnectionTrait,
    post_id: uuid::Uuid,
) -> Result<Option<posts::Model>, DbErr> {
    active_posts()
        .filter(posts::Column::Id.eq(post_id))
        .one(db)
        .await
}

/// echobackend's `GetPostByID` + `ensurePostInteractable`: whether a post can
/// receive engagement (like, comment, bookmark, view, report). A hidden post
/// or one whose author is soft-deleted is reported as missing.
pub async fn interactable_post_exists(
    db: &impl ConnectionTrait,
    post_id: uuid::Uuid,
) -> Result<bool, DbErr> {
    Ok(active_posts()
        .filter(posts::Column::Id.eq(post_id))
        .filter(posts::Column::HiddenAt.is_null())
        .one(db)
        .await?
        .is_some())
}

/// Full filter for `GET /api/posts`, mirroring echobackend's `PostQueryFilter`
/// (`internal/dto/post.go` + `repository.GetPostsFiltered`). There is no
/// `published` filter: the public list only ever shows published posts.
pub struct PostsFilter<'a> {
    pub offset: i64,
    pub limit: i64,
    pub search: Option<&'a str>,
    pub sort_by: Option<&'a str>,
    pub sort_order: Option<SortDirection>,
    pub start_date: Option<chrono::DateTime<Utc>>,
    pub end_date: Option<chrono::DateTime<Utc>>,
    pub created_by: Option<uuid::Uuid>,
    pub tags: Vec<String>,
}

/// `posts.id IN (SELECT posts_to_tags.post_id FROM posts_to_tags JOIN tags ON
/// tags.id = posts_to_tags.tag_id WHERE tags.name IN (..))`, echobackend's
/// tag filter: OR across names, exact match, each post at most once.
fn tagged_with_any(tag_names: &[String]) -> SimpleExpr {
    posts::Column::Id.in_subquery(
        sea_orm::sea_query::Query::select()
            .column((posts_to_tags::Entity, posts_to_tags::Column::PostId))
            .from(posts_to_tags::Entity)
            .inner_join(
                tags::Entity,
                Expr::col((tags::Entity, tags::Column::Id))
                    .equals((posts_to_tags::Entity, posts_to_tags::Column::TagId)),
            )
            .and_where(Expr::col((tags::Entity, tags::Column::Name)).is_in(tag_names.to_vec()))
            .to_owned(),
    )
}

pub async fn get_all_posts(
    db: &DatabaseConnection,
    filter: PostsFilter<'_>,
) -> Result<(Vec<Post>, i64), DbErr> {
    // echobackend's `applyPostFilters`: posts by soft-deleted users are
    // hidden, and the public list only shows published, non-hidden posts
    // (authors see their drafts through `/api/posts/me`).
    let mut query = active_posts()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null());

    if let Some(search) = filter.search.filter(|s| !s.is_empty()) {
        // echobackend: `posts.title ILIKE '%search%'` on the raw value.
        query = query
            .filter(Expr::col((posts::Entity, posts::Column::Title)).ilike(format!("%{search}%")));
    }
    // echobackend compares `created_at >= start_date` / `<= end_date`
    // directly, so a bare date means midnight at the start of that day.
    if let Some(start) = filter.start_date {
        query = query.filter(posts::Column::CreatedAt.gte(start));
    }
    if let Some(end) = filter.end_date {
        query = query.filter(posts::Column::CreatedAt.lte(end));
    }
    if let Some(created_by) = filter.created_by {
        query = query.filter(posts::Column::CreatedBy.eq(created_by));
    }
    if !filter.tags.is_empty() {
        query = query.filter(tagged_with_any(&filter.tags));
    }

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by(
            validate_order_field(filter.sort_by),
            get_order_dir(filter.sort_order),
        )
        .limit(Ord::max(filter.limit, 0) as u64)
        .offset(Ord::max(filter.offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

/// echobackend `GetPostByID` (admin detail and `/api/posts/me/{id}`): full
/// body, drafts and hidden posts included, `None` when the post is missing or
/// its author is soft-deleted.
pub async fn get_post_by_id(
    db: &DatabaseConnection,
    id: uuid::Uuid,
) -> Result<Option<Post>, DbErr> {
    let post = find_active_post(db, id).await?;

    match post {
        Some(post) => {
            let user = post.find_related(users::Entity).one(db).await?;
            let tags = post.find_related(tags::Entity).all(db).await?;
            Ok(Some(hydrate_post(&post, user, tags, false).await?))
        }
        None => Ok(None),
    }
}

pub async fn get_posts_by_username(
    db: &DatabaseConnection,
    username: &str,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Post>, i64), DbErr> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let Some(user) = user else {
        return Ok((Vec::new(), 0));
    };

    // echobackend's GetPostByUsername: public, so only published posts that
    // are not hidden (drafts stay private to `/api/posts/me`).
    let query = user
        .clone()
        .find_related(posts::Entity)
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

/// echobackend `GetPostsByCreatedBy` (`GET /api/posts/me`): the author's own
/// posts, drafts and hidden ones included. `search` filters on
/// `title ILIKE '%search%'`, `published` on the published flag.
pub async fn get_posts_by_created_by(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
    offset: i64,
    limit: i64,
    search: Option<&str>,
    published: Option<bool>,
) -> Result<(Vec<Post>, i64), DbErr> {
    let mut query = active_posts().filter(posts::Column::CreatedBy.eq(user_id));
    if let Some(search) = search.filter(|s| !s.is_empty()) {
        query = query
            .filter(Expr::col((posts::Entity, posts::Column::Title)).ilike(format!("%{search}%")));
    }
    if let Some(published) = published {
        query = query.filter(posts::Column::Published.eq(published));
    }

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

/// Mirrors echobackend's `GetPostsForYou`: posts authored by `user_id` or by
/// anyone `user_id` follows, published, newest first.
pub async fn get_posts_for_you(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Post>, i64), DbErr> {
    let following_ids = crate::entities::user_follows::Entity::find()
        .select_only()
        .column(crate::entities::user_follows::Column::FollowingId)
        .filter(crate::entities::user_follows::Column::FollowerId.eq(user_id))
        .filter(crate::entities::user_follows::Column::DeletedAt.is_null())
        .into_query();

    let query = active_posts()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null())
        .filter(
            posts::Column::CreatedBy
                .eq(user_id)
                .or(posts::Column::CreatedBy.in_subquery(following_ids)),
        );

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

/// Cached for `CACHE_TTL_SECONDS` under `posts:random:<limit>` when Redis is
/// configured, like echobackend's `GetPostsRandom`.
pub async fn get_random_posts(db: &DatabaseConnection, limit: i64) -> Result<Vec<Post>, DbErr> {
    let cache_key = cache::get().map(|c| c.build_key(&["posts", "random", &limit.to_string()]));
    if let Some(posts) = read_cached::<Vec<Post>>(cache_key.as_deref()).await {
        return Ok(posts);
    }

    let post_models = active_posts()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null())
        .order_by(sea_orm::sea_query::Expr::cust("RANDOM()"), Order::Asc)
        .limit(Ord::max(limit, 0) as u64)
        .all(db)
        .await?;

    let posts = hydrate_posts(db, post_models, true).await?;
    write_cached(cache_key.as_deref(), &posts).await;
    Ok(posts)
}

/// echobackend caches trending posts wrapped in its `trendingPostsCacheEntry`
/// struct (`json:"posts"`), so the shared key holds `{"posts": [...]}`.
/// `Posts` (the casing older axum builds wrote) is still read until it expires.
#[derive(Serialize, Deserialize)]
struct TrendingPostsCacheEntry {
    #[serde(alias = "Posts")]
    posts: Vec<Post>,
}

/// Cached for `CACHE_TTL_SECONDS` under `posts:trending:<limit>` when Redis is
/// configured, like echobackend's `GetPostsTrending`.
pub async fn get_trending_posts(db: &DatabaseConnection, limit: i64) -> Result<Vec<Post>, DbErr> {
    let cache_key = cache::get().map(|c| c.build_key(&["posts", "trending", &limit.to_string()]));
    if let Some(entry) = read_cached::<TrendingPostsCacheEntry>(cache_key.as_deref()).await {
        return Ok(entry.posts);
    }

    // Posts by soft-deleted users are hidden, like echobackend's `activePostUserJoin`.
    let post_models = active_posts()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null())
        // Promotional posts stay published and discoverable via search, tags
        // and profiles — they just never trend. Values come from the shared
        // anti-spam constants, so the list cannot drift from echobackend's.
        .filter(Expr::cust(format!(
            "NOT EXISTS (SELECT 1 FROM posts_to_tags ptt INNER JOIN tags t ON t.id = ptt.tag_id WHERE ptt.post_id = posts.id AND LOWER(t.name) IN ({}))",
            crate::domain::antispam::promo_tags_sql_in()
        )))
        // The counters are nullable; without COALESCE a single NULL makes the
        // whole score NULL, and Postgres sorts NULLs first under DESC.
        .order_by(
            Expr::cust(
                "COALESCE(posts.like_count, 0) * 2 + COALESCE(posts.bookmark_count, 0) * 2 + COALESCE(posts.view_count, 0)",
            ),
            Order::Desc,
        )
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .all(db)
        .await?;

    let entry = TrendingPostsCacheEntry {
        posts: hydrate_posts(db, post_models, true).await?,
    };
    write_cached(cache_key.as_deref(), &entry).await;
    Ok(entry.posts)
}

/// A cache error is treated as a miss: the database stays the source of truth.
async fn read_cached<T: serde::de::DeserializeOwned>(key: Option<&str>) -> Option<T> {
    let (cache, key) = (cache::get()?, key?);
    match cache.get_json(key).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(key, error = %err, "post cache: read failed");
            None
        }
    }
}

async fn write_cached<T: Serialize>(key: Option<&str>, value: &T) {
    let (Some(cache), Some(key)) = (cache::get(), key) else {
        return;
    };
    if let Err(err) = cache.set_json(key, value).await {
        tracing::warn!(key, error = %err, "post cache: write failed");
    }
}

pub async fn get_posts_for_sitemap(
    db: &DatabaseConnection,
    limit: i64,
) -> Result<Vec<SitemapPost>, DbErr> {
    #[derive(FromQueryResult)]
    struct SitemapRow {
        username: Option<String>,
        slug: String,
        created_at: Option<chrono::DateTime<chrono::FixedOffset>>,
        updated_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    }

    let rows: Vec<SitemapRow> =
        SitemapRow::find_by_statement(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            r#"
            SELECT
                u.username,
                p.slug,
                p.created_at,
                p.updated_at
            FROM posts p
            INNER JOIN users u ON u.id = p.created_by
            WHERE p.published = true AND p.hidden_at IS NULL AND u.deleted_at IS NULL
            ORDER BY p.created_at DESC
            LIMIT $1
            "#,
            vec![Ord::max(limit, 0).into()],
        ))
        .all(db)
        .await?;

    let sitemap = rows
        .into_iter()
        .map(|row| SitemapPost {
            username: row.username,
            slug: row.slug,
            created_at: row.created_at.map(|dt| dt.with_timezone(&Utc)),
            updated_at: row.updated_at.map(|dt| dt.with_timezone(&Utc)),
        })
        .collect();

    Ok(sitemap)
}

pub async fn get_post_by_username_and_slug(
    db: &DatabaseConnection,
    username: &str,
    slug: &str,
) -> Result<Option<Post>, DbErr> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let Some(user) = user else {
        return Ok(None);
    };

    let post = user
        .find_related(posts::Entity)
        .filter(posts::Column::Slug.eq(slug))
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null())
        .one(db)
        .await?;

    match post {
        Some(post) => {
            let user = post.find_related(users::Entity).one(db).await?;
            let tags = post.find_related(tags::Entity).all(db).await?;
            Ok(Some(hydrate_post(&post, user, tags, false).await?))
        }
        None => Ok(None),
    }
}

/// echobackend `FindOrCreateByName`: read first, then
/// `INSERT .. ON CONFLICT (name) DO NOTHING` and re-read, so two requests that
/// introduce the same new tag concurrently both succeed instead of the loser
/// hitting the unique index `idx_tags_name`.
async fn find_or_create_tag(db: &DatabaseConnection, name: &str) -> Result<tags::Model, DbErr> {
    if let Some(tag) = find_tag_by_name(db, name).await? {
        return Ok(tag);
    }

    tags::Entity::insert(tags::ActiveModel {
        name: Set(name.to_string()),
        created_at: Set(Some(Utc::now().into())),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(tags::Column::Name)
            .do_nothing()
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    // Either our insert or the concurrent winner's row is there now.
    find_tag_by_name(db, name)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound(format!("tag {name:?} not found after insert")))
}

async fn find_tag_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<tags::Model>, DbErr> {
    tags::Entity::find()
        .filter(tags::Column::Name.eq(name))
        .one(db)
        .await
}

fn normalize_tag_names(tag_names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for name in tag_names
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        if seen.insert(name.to_string()) {
            normalized.push(name.to_string());
        }
    }

    normalized
}

async fn replace_post_tags(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    tag_names: &[String],
) -> Result<(), DbErr> {
    posts_to_tags::Entity::delete_many()
        .filter(posts_to_tags::Column::PostId.eq(post_id))
        .exec(db)
        .await?;

    for name in normalize_tag_names(tag_names) {
        let tag = find_or_create_tag(db, &name).await?;
        posts_to_tags::ActiveModel {
            post_id: Set(post_id),
            tag_id: Set(tag.id),
        }
        .insert(db)
        .await?;
    }

    Ok(())
}

pub struct CreatePostInput {
    pub title: String,
    pub photo_url: Option<String>,
    pub slug: String,
    pub body: String,
    pub published: bool,
    pub tags: Vec<String>,
}

pub struct UpdatePostInput {
    pub title: Option<String>,
    pub photo_url: Option<String>,
    pub slug: Option<String>,
    pub body: Option<String>,
    pub published: Option<bool>,
    pub tags: Option<Vec<String>>,
}

/// Writing a post can fail for a policy reason, not just a database error.
#[derive(Debug)]
pub enum PostWriteError {
    /// Content tripped the anti-spam guard (`crate::domain::antispam`); carries the
    /// user-facing reason surfaced through the 422 envelope.
    SpamRejected(String),
    /// The post is missing, its author is soft-deleted, or (owner-scoped
    /// writes) it belongs to someone else (echobackend `ErrPostNotFound`).
    NotFound,
    /// No-op owner update on someone else's post (echobackend `ErrNotAuthor`).
    NotAuthor,
    Db(DbErr),
}

impl From<DbErr> for PostWriteError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

pub async fn create_post(
    db: &DatabaseConnection,
    input: CreatePostInput,
    creator_id: uuid::Uuid,
) -> Result<Post, PostWriteError> {
    // Proactive spam guard: marketing spam is rejected with a 422 that tells
    // the author the rule, instead of polluting feeds until someone reports it.
    if let Some(reason) =
        crate::domain::antispam::post_reject_reason(&input.title, &input.body, &input.tags)
    {
        return Err(PostWriteError::SpamRejected(reason));
    }

    let now = Utc::now();
    let post = posts::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        title: Set(input.title),
        created_by: Set(creator_id),
        body: Set(Some(input.body)),
        slug: Set(input.slug),
        photo_url: Set(Some(input.photo_url.unwrap_or_default())),
        published: Set(Some(input.published)),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
        view_count: Set(Some(0)),
        like_count: Set(Some(0)),
        bookmark_count: Set(Some(0)),
        ..Default::default()
    }
    .insert(db)
    .await?;

    replace_post_tags(db, post.id, &input.tags).await?;
    let user = post.find_related(users::Entity).one(db).await?;
    let tags = post.find_related(tags::Entity).all(db).await?;
    hydrate_post(&post, user, tags, false)
        .await
        .map_err(PostWriteError::from)
}

/// echobackend `IsAuthor`: `None` when the post is missing or its author is
/// soft-deleted (`GetPostByID`), otherwise whether `user_id` wrote it.
pub async fn is_author(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<bool>, DbErr> {
    let post = find_active_post(db, post_id).await?;

    Ok(post.map(|post| post.created_by == user_id))
}

/// The scalar columns an update writes, mirroring echobackend's `updates`
/// map: `title`, `body`, `slug` and `photo_url` only when present and not the
/// exact empty string (whitespace-only values are written, `""` never clears
/// a field), `published` whenever present.
#[derive(Debug, Default, PartialEq)]
struct PostColumnUpdates {
    title: Option<String>,
    body: Option<String>,
    slug: Option<String>,
    photo_url: Option<String>,
    published: Option<bool>,
}

impl PostColumnUpdates {
    fn from_input(input: &UpdatePostInput) -> Self {
        let non_empty = |value: &Option<String>| value.clone().filter(|v| !v.is_empty());
        Self {
            title: non_empty(&input.title),
            body: non_empty(&input.body),
            slug: non_empty(&input.slug),
            photo_url: non_empty(&input.photo_url),
            published: input.published,
        }
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Same guard as creation; only submitted fields are checked (absent fields
/// behave as empty strings, which never trigger any rule).
fn check_update_spam(input: &UpdatePostInput) -> Result<(), PostWriteError> {
    match crate::domain::antispam::post_reject_reason(
        input.title.as_deref().unwrap_or(""),
        input.body.as_deref().unwrap_or(""),
        input.tags.as_deref().unwrap_or_default(),
    ) {
        Some(reason) => Err(PostWriteError::SpamRejected(reason)),
        None => Ok(()),
    }
}

/// Updates a post, mirroring echobackend's `UpdatePost` (`owner: None`, admin
/// `PUT /api/posts/{id}`) and `UpdateMyPost` (`owner: Some(user)`, `PUT
/// /api/posts/me/{id}`):
/// 1. the anti-spam guard runs first (422), before any DB access;
/// 2. with no column to write and no `tags`, the current post is returned
///    untouched (`updated_at` is not bumped): missing post (or soft-deleted
///    author) → `NotFound`, someone else's post → `NotAuthor`;
/// 3. otherwise `UPDATE .. WHERE id = ? [AND created_by = ?]` sets the columns
///    plus `updated_at` (only when a column changes, like GORM `Updates`);
///    0 rows → `NotFound`, so a non-owner gets 404, not 403;
/// 4. the post is reloaded (owner-scoped for `UpdateMyPost`) and `tags`
///    `Some(..)`, even empty, replaces the whole association set.
pub async fn update_post(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    owner: Option<uuid::Uuid>,
    input: UpdatePostInput,
) -> Result<Post, PostWriteError> {
    check_update_spam(&input)?;

    let updates = PostColumnUpdates::from_input(&input);
    if updates.is_empty() && input.tags.is_none() {
        let post = get_post_by_id(db, post_id)
            .await?
            .ok_or(PostWriteError::NotFound)?;
        if owner.is_some_and(|owner| post.created_by != owner) {
            return Err(PostWriteError::NotAuthor);
        }
        return Ok(post);
    }

    if !updates.is_empty() {
        let mut update = posts::Entity::update_many()
            .col_expr(posts::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(posts::Column::Id.eq(post_id));
        if let Some(owner) = owner {
            update = update.filter(posts::Column::CreatedBy.eq(owner));
        }
        if let Some(title) = updates.title {
            update = update.col_expr(posts::Column::Title, Expr::value(title));
        }
        if let Some(body) = updates.body {
            update = update.col_expr(posts::Column::Body, Expr::value(body));
        }
        if let Some(slug) = updates.slug {
            update = update.col_expr(posts::Column::Slug, Expr::value(slug));
        }
        if let Some(photo_url) = updates.photo_url {
            update = update.col_expr(posts::Column::PhotoUrl, Expr::value(photo_url));
        }
        if let Some(published) = updates.published {
            update = update.col_expr(posts::Column::Published, Expr::value(published));
        }
        if update.exec(db).await?.rows_affected == 0 {
            return Err(PostWriteError::NotFound);
        }
    }

    let mut reload = posts::Entity::find_by_id(post_id);
    if let Some(owner) = owner {
        reload = reload.filter(posts::Column::CreatedBy.eq(owner));
    }
    let post = reload.one(db).await?.ok_or(PostWriteError::NotFound)?;

    if let Some(tag_names) = &input.tags {
        replace_post_tags(db, post.id, tag_names).await?;
    }

    let user = post.find_related(users::Entity).one(db).await?;
    let tags = post.find_related(tags::Entity).all(db).await?;
    Ok(hydrate_post(&post, user, tags, false).await?)
}

/// Hard delete (echobackend `DeletePostByID` / `DeletePostByOwner`; child rows
/// go through `ON DELETE CASCADE`). With `owner` the delete is scoped to that
/// author, so a non-owner gets `false` (404) rather than 403.
pub async fn delete_post(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    owner: Option<uuid::Uuid>,
) -> Result<bool, DbErr> {
    let mut delete = posts::Entity::delete_many().filter(posts::Column::Id.eq(post_id));
    if let Some(owner) = owner {
        delete = delete.filter(posts::Column::CreatedBy.eq(owner));
    }
    Ok(delete.exec(db).await?.rows_affected > 0)
}

/// echobackend `GetPostsByTag`: published, non-hidden posts carrying the tag
/// named exactly `tag_name`, authors not soft-deleted.
pub async fn get_posts_by_tag(
    db: &DatabaseConnection,
    tag_name: &str,
    offset: i64,
    limit: i64,
    order_by: Option<&str>,
    order_direction: Option<SortDirection>,
) -> Result<(Vec<Post>, i64), DbErr> {
    if tag_name.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let query = active_posts()
        .join(JoinType::InnerJoin, posts::Relation::PostsToTags.def())
        .join(JoinType::InnerJoin, posts_to_tags::Relation::Tags.def())
        .filter(tags::Column::Name.eq(tag_name))
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::HiddenAt.is_null());

    let total = query.clone().count(db).await? as i64;
    // Same whitelist/fallback as GET /api/posts (echobackend PostQueryFilter).
    let post_models = query
        .order_by(
            validate_order_field(order_by),
            get_order_dir(order_direction),
        )
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tag_names_trims_filters_empty_and_deduplicates() {
        let input = vec![
            " rust ".to_string(),
            "".to_string(),
            "axum".to_string(),
            "rust".to_string(),
            "   ".to_string(),
            "sea-orm".to_string(),
        ];

        let normalized = normalize_tag_names(&input);

        assert_eq!(normalized, vec!["rust", "axum", "sea-orm"]);
    }

    #[test]
    fn test_validate_order_field_mappings() {
        assert!(matches!(
            validate_order_field(Some("id")),
            posts::Column::Id
        ));
        assert!(matches!(
            validate_order_field(Some("title")),
            posts::Column::Title
        ));
        assert!(matches!(
            validate_order_field(Some("updated_at")),
            posts::Column::UpdatedAt
        ));
        assert!(matches!(
            validate_order_field(Some("view_count")),
            posts::Column::ViewCount
        ));
        assert!(matches!(
            validate_order_field(Some("like_count")),
            posts::Column::LikeCount
        ));
        assert!(matches!(
            validate_order_field(Some("bookmark_count")),
            posts::Column::BookmarkCount
        ));
        assert!(matches!(
            validate_order_field(Some("unknown")),
            posts::Column::CreatedAt
        ));
        assert!(matches!(
            validate_order_field(None),
            posts::Column::CreatedAt
        ));
    }

    #[test]
    fn tags_filter_is_a_subquery_not_a_join() {
        // A post carrying several requested tags must be listed and counted
        // once, so the filter must not multiply rows with a join.
        let sql = active_posts()
            .filter(tagged_with_any(&["rust".to_string(), "go".to_string()]))
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(
            str::contains(
                &sql,
                r#""posts"."id" IN (SELECT "posts_to_tags"."post_id" FROM "posts_to_tags" INNER JOIN "tags""#
            ),
            "{sql}"
        );
        assert!(
            str::contains(&sql, r#""tags"."name" IN ('rust', 'go')"#),
            "{sql}"
        );
        assert!(str::contains(&sql, r#"INNER JOIN "users""#), "{sql}");
        assert!(
            str::contains(&sql, r#""users"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert_eq!(str::matches(&sql, "JOIN").count(), 2, "{sql}");
    }

    fn update_input() -> UpdatePostInput {
        UpdatePostInput {
            title: None,
            photo_url: None,
            slug: None,
            body: None,
            published: None,
            tags: None,
        }
    }

    #[test]
    fn update_columns_follow_echobackend_rules() {
        // Nothing sent: a no-op (no UPDATE, updated_at untouched).
        assert!(PostColumnUpdates::from_input(&update_input()).is_empty());

        // Exact "" is ignored (photo_url can never be cleared)...
        let input = UpdatePostInput {
            title: Some(String::new()),
            photo_url: Some(String::new()),
            ..update_input()
        };
        assert!(PostColumnUpdates::from_input(&input).is_empty());

        // ...while whitespace-only values are written, and `published` counts.
        let input = UpdatePostInput {
            title: Some("  ".into()),
            published: Some(false),
            ..update_input()
        };
        let updates = PostColumnUpdates::from_input(&input);
        assert_eq!(updates.title.as_deref(), Some("  "));
        assert_eq!(updates.published, Some(false));
        assert!(!updates.is_empty());
    }

    #[test]
    fn test_get_order_dir() {
        assert_eq!(get_order_dir(Some(SortDirection::Asc)), Order::Asc);
        assert_eq!(get_order_dir(Some(SortDirection::Desc)), Order::Desc);
        assert_eq!(get_order_dir(None), Order::Desc);
    }
}
