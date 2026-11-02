use super::read::hydrate_post;
use super::{find_active_post, get_post_by_id};
use crate::entities::{posts, posts_to_tags, tags, users};
use crate::models::post::Post;
use chrono::Utc;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, ModelTrait, QueryFilter,
    Set,
};
use std::collections::HashSet;

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
}
