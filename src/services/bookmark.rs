use crate::entities::{bookmark_folders, post_bookmarks, posts};
use crate::models::bookmark::{BookmarkFolderResponse, BookmarkResponse, ToggleBookmarkResponse};
use crate::models::post::Post;
use crate::services::user_hydration;
use chrono::Utc;
use sea_orm::sea_query::{Query, SimpleExpr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Debug)]
pub enum BookmarkError {
    Db(DbErr),
    PostNotFound,
    BookmarkNotFound,
    FolderNotFound,
}

impl From<DbErr> for BookmarkError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

async fn folder_count(db: &DatabaseConnection, folder_id: Uuid) -> Result<i64, DbErr> {
    Ok(post_bookmarks::Entity::find()
        .filter(post_bookmarks::Column::FolderId.eq(folder_id))
        .count(db)
        .await? as i64)
}

/// `bookmark_count` for several folders in one query.
async fn folder_counts(
    db: &DatabaseConnection,
    folder_ids: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, i64>, DbErr> {
    if folder_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Option<Uuid>, i64)> = post_bookmarks::Entity::find()
        .select_only()
        .column(post_bookmarks::Column::FolderId)
        .column_as(post_bookmarks::Column::Id.count(), "count")
        .filter(post_bookmarks::Column::FolderId.is_in(folder_ids.iter().copied()))
        .group_by(post_bookmarks::Column::FolderId)
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(folder_id, count)| folder_id.map(|id| (id, count)))
        .collect())
}

async fn find_folder(
    db: &DatabaseConnection,
    folder_id: Uuid,
    user_id: Uuid,
) -> Result<Option<bookmark_folders::Model>, DbErr> {
    bookmark_folders::Entity::find_by_id(folder_id)
        .filter(bookmark_folders::Column::UserId.eq(user_id))
        .one(db)
        .await
}

async fn folder_response(
    db: &DatabaseConnection,
    folder: bookmark_folders::Model,
) -> Result<BookmarkFolderResponse, DbErr> {
    let count = folder_count(db, folder.id).await?;
    Ok(BookmarkFolderResponse::from_entity(folder, count))
}

/// Builds the responses for `bookmarks` with a fixed number of queries.
/// Like echobackend's `Preload("Post")` + `Preload("Post.User",
/// preloadUserBrief)`: `post` carries the FULL body (no truncation), its
/// author brief `{id, username, image}` (left out when the author is
/// soft-deleted) and no tags.
async fn hydrate_bookmarks(
    db: &DatabaseConnection,
    bookmarks: Vec<post_bookmarks::Model>,
) -> Result<Vec<BookmarkResponse>, DbErr> {
    if bookmarks.is_empty() {
        return Ok(Vec::new());
    }

    let post_ids: HashSet<Uuid> = bookmarks.iter().map(|b| b.post_id).collect();
    let posts_by_id: HashMap<Uuid, posts::Model> = posts::Entity::find()
        .filter(posts::Column::Id.is_in(post_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|post| (post.id, post))
        .collect();
    let authors =
        user_hydration::load_user_brief_map(db, posts_by_id.values().map(|p| p.created_by)).await?;

    let folder_ids: HashSet<Uuid> = bookmarks.iter().filter_map(|b| b.folder_id).collect();
    let folders_by_id: HashMap<Uuid, bookmark_folders::Model> = if folder_ids.is_empty() {
        HashMap::new()
    } else {
        bookmark_folders::Entity::find()
            .filter(bookmark_folders::Column::Id.is_in(folder_ids.iter().copied()))
            .all(db)
            .await?
            .into_iter()
            .map(|folder| (folder.id, folder))
            .collect()
    };
    let counts = folder_counts(db, &folder_ids).await?;

    Ok(bookmarks
        .into_iter()
        .map(|bookmark| {
            let post = posts_by_id.get(&bookmark.post_id).cloned().map(|model| {
                let author = model.created_by;
                let mut post = Post::from_entity(model, None, Vec::new(), false);
                post.user = authors.get(&author).cloned();
                post
            });
            let folder = bookmark
                .folder_id
                .and_then(|id| folders_by_id.get(&id).cloned())
                .map(|folder| {
                    let count = counts.get(&folder.id).copied().unwrap_or(0);
                    BookmarkFolderResponse::from_entity(folder, count)
                });
            BookmarkResponse::from_entity(bookmark, post, folder)
        })
        .collect())
}

async fn hydrate_bookmark(
    db: &DatabaseConnection,
    bookmark: post_bookmarks::Model,
) -> Result<BookmarkResponse, DbErr> {
    hydrate_bookmarks(db, vec![bookmark])
        .await?
        .pop()
        .ok_or_else(|| DbErr::Custom("bookmark hydration returned no row".to_string()))
}

/// echobackend `ToggleBookmark`, in its order:
/// 1. the post must exist with an active author (hidden posts pass);
/// 2. an existing bookmark is removed, whatever the post's state or the body's
///    `folder_id`/`name`/`notes`;
/// 3. only when adding: a hidden post is "not found" (`ensurePostInteractable`);
/// 4. a `folder_id` must be one of the user's folders;
/// 5. insert.
pub async fn toggle_bookmark(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
    folder_id: Option<Uuid>,
    name: Option<String>,
    notes: Option<String>,
) -> Result<ToggleBookmarkResponse, BookmarkError> {
    let Some(post) = crate::services::post::find_active_post(db, post_id).await? else {
        return Err(BookmarkError::PostNotFound);
    };

    if let Some(existing) = post_bookmarks::Entity::find()
        .filter(post_bookmarks::Column::UserId.eq(user_id))
        .filter(post_bookmarks::Column::PostId.eq(post_id))
        .one(db)
        .await?
    {
        post_bookmarks::Entity::delete_by_id(existing.id)
            .exec(db)
            .await?;
        return Ok(ToggleBookmarkResponse {
            action: "removed".to_string(),
            bookmark: None,
        });
    }

    if post.hidden_at.is_some() {
        return Err(BookmarkError::PostNotFound);
    }

    if let Some(folder_id) = folder_id
        && find_folder(db, folder_id, user_id).await?.is_none()
    {
        return Err(BookmarkError::FolderNotFound);
    }

    let now = Utc::now();
    let bookmark = post_bookmarks::ActiveModel {
        id: Set(Uuid::now_v7()),
        post_id: Set(post_id),
        user_id: Set(user_id),
        folder_id: Set(folder_id),
        name: Set(name),
        notes: Set(notes),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
    }
    .insert(db)
    .await?;

    Ok(ToggleBookmarkResponse {
        action: "added".to_string(),
        bookmark: Some(hydrate_bookmark(db, bookmark).await?),
    })
}

/// `posts.id` of every hidden post. Bookmarks of hidden posts stay in the
/// table (they reappear when the post is unhidden) but are left out of the
/// list and its total, like echobackend's
/// `post_id NOT IN (SELECT id FROM posts WHERE hidden_at IS NOT NULL)`.
fn not_on_hidden_post() -> SimpleExpr {
    post_bookmarks::Column::PostId.not_in_subquery(
        Query::select()
            .column(posts::Column::Id)
            .from(posts::Entity)
            .and_where(posts::Column::HiddenAt.is_not_null())
            .to_owned(),
    )
}

pub async fn get_bookmarks_by_user(
    db: &DatabaseConnection,
    user_id: Uuid,
    folder_id: Option<Option<Uuid>>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<BookmarkResponse>, i64), BookmarkError> {
    let mut query = post_bookmarks::Entity::find()
        .filter(post_bookmarks::Column::UserId.eq(user_id))
        .filter(not_on_hidden_post());
    if let Some(folder_filter) = folder_id {
        match folder_filter {
            Some(folder_id) => query = query.filter(post_bookmarks::Column::FolderId.eq(folder_id)),
            None => query = query.filter(post_bookmarks::Column::FolderId.is_null()),
        }
    }
    let total = query.clone().count(db).await? as i64;
    let models = query
        .order_by_desc(post_bookmarks::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;
    Ok((hydrate_bookmarks(db, models).await?, total))
}

pub async fn update_bookmark(
    db: &DatabaseConnection,
    bookmark_id: Uuid,
    user_id: Uuid,
    name: Option<String>,
    notes: Option<String>,
) -> Result<BookmarkResponse, BookmarkError> {
    let Some(bookmark) = post_bookmarks::Entity::find_by_id(bookmark_id)
        .filter(post_bookmarks::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Err(BookmarkError::BookmarkNotFound);
    };
    let mut active = bookmark.into_active_model();
    if name.is_some() {
        active.name = Set(name);
    }
    if notes.is_some() {
        active.notes = Set(notes);
    }
    active.updated_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;
    Ok(hydrate_bookmark(db, updated).await?)
}

pub async fn move_bookmark(
    db: &DatabaseConnection,
    bookmark_id: Uuid,
    user_id: Uuid,
    folder_id: Option<Uuid>,
) -> Result<BookmarkResponse, BookmarkError> {
    if let Some(folder_id) = folder_id
        && find_folder(db, folder_id, user_id).await?.is_none()
    {
        return Err(BookmarkError::FolderNotFound);
    }
    let Some(bookmark) = post_bookmarks::Entity::find_by_id(bookmark_id)
        .filter(post_bookmarks::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Err(BookmarkError::BookmarkNotFound);
    };
    let mut active = bookmark.into_active_model();
    active.folder_id = Set(folder_id);
    active.updated_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;
    Ok(hydrate_bookmark(db, updated).await?)
}

pub async fn create_folder(
    db: &DatabaseConnection,
    user_id: Uuid,
    name: String,
    description: Option<String>,
) -> Result<BookmarkFolderResponse, BookmarkError> {
    let now = Utc::now();
    let folder = bookmark_folders::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        name: Set(name),
        description: Set(description),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
    }
    .insert(db)
    .await?;
    Ok(folder_response(db, folder).await?)
}

pub async fn get_folders_by_user(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Vec<BookmarkFolderResponse>, BookmarkError> {
    let folders = bookmark_folders::Entity::find()
        .filter(bookmark_folders::Column::UserId.eq(user_id))
        .order_by_desc(bookmark_folders::Column::CreatedAt)
        .all(db)
        .await?;
    let mut out = Vec::with_capacity(folders.len());
    for folder in folders {
        out.push(folder_response(db, folder).await?);
    }
    Ok(out)
}

pub async fn update_folder(
    db: &DatabaseConnection,
    folder_id: Uuid,
    user_id: Uuid,
    name: Option<String>,
    description: Option<String>,
) -> Result<BookmarkFolderResponse, BookmarkError> {
    let Some(folder) = find_folder(db, folder_id, user_id).await? else {
        return Err(BookmarkError::FolderNotFound);
    };
    let mut active = folder.into_active_model();
    if let Some(name) = name {
        active.name = Set(name);
    }
    if description.is_some() {
        active.description = Set(description);
    }
    active.updated_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;
    Ok(folder_response(db, updated).await?)
}

pub async fn delete_folder(
    db: &DatabaseConnection,
    folder_id: Uuid,
    user_id: Uuid,
) -> Result<(), BookmarkError> {
    let result = bookmark_folders::Entity::delete_many()
        .filter(bookmark_folders::Column::Id.eq(folder_id))
        .filter(bookmark_folders::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(BookmarkError::FolderNotFound);
    }
    Ok(())
}
