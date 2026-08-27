use crate::entities::{bookmark_folders, post_bookmarks, posts};
use crate::models::bookmark::{BookmarkFolderResponse, BookmarkResponse, ToggleBookmarkResponse};
use crate::models::post::Post;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
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

async fn post_exists(db: &DatabaseConnection, post_id: Uuid) -> Result<bool, DbErr> {
    Ok(posts::Entity::find_by_id(post_id)
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some())
}

async fn folder_count(db: &DatabaseConnection, folder_id: Uuid) -> Result<i64, DbErr> {
    Ok(post_bookmarks::Entity::find()
        .filter(post_bookmarks::Column::FolderId.eq(folder_id))
        .count(db)
        .await? as i64)
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

async fn hydrate_bookmark(
    db: &DatabaseConnection,
    bookmark: post_bookmarks::Model,
) -> Result<BookmarkResponse, DbErr> {
    let post = bookmark
        .clone()
        .find_related(posts::Entity)
        .one(db)
        .await?
        .map(|post_model| Post::from_entity(post_model, None, Vec::new(), true));
    let folder = match bookmark
        .clone()
        .find_related(bookmark_folders::Entity)
        .one(db)
        .await?
    {
        Some(folder) => Some(folder_response(db, folder).await?),
        None => None,
    };
    Ok(BookmarkResponse::from_entity(bookmark, post, folder))
}

pub async fn toggle_bookmark(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
    folder_id: Option<Uuid>,
    name: Option<String>,
    notes: Option<String>,
) -> Result<ToggleBookmarkResponse, BookmarkError> {
    if !post_exists(db, post_id).await? {
        return Err(BookmarkError::PostNotFound);
    }

    if let Some(folder_id) = folder_id
        && find_folder(db, folder_id, user_id).await?.is_none()
    {
        return Err(BookmarkError::FolderNotFound);
    }

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

    let now = Utc::now();
    let bookmark = post_bookmarks::ActiveModel {
        id: Set(Uuid::new_v4()),
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

pub async fn get_bookmarks_by_user(
    db: &DatabaseConnection,
    user_id: Uuid,
    folder_id: Option<Option<Uuid>>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<BookmarkResponse>, i64), BookmarkError> {
    let mut query =
        post_bookmarks::Entity::find().filter(post_bookmarks::Column::UserId.eq(user_id));
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
    let mut out = Vec::with_capacity(models.len());
    for model in models {
        out.push(hydrate_bookmark(db, model).await?);
    }
    Ok((out, total))
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
        id: Set(Uuid::new_v4()),
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
