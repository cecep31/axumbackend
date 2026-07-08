use crate::entities::{post_views, posts};
use crate::models::post_view::{
    MyPostsAnalyticsResponse, MyPostsLikesByMonthResponse, PostViewResponse, PostViewStats,
    ViewStatusResponse,
};
use crate::models::user::UserResponse;
use crate::services::user_hydration;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug)]
pub enum PostViewError {
    Db(DbErr),
    PostNotFound,
}

impl From<DbErr> for PostViewError {
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

async fn has_user_viewed(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<bool, DbErr> {
    Ok(post_views::Entity::find()
        .filter(post_views::Column::PostId.eq(post_id))
        .filter(post_views::Column::UserId.eq(user_id))
        .filter(post_views::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some())
}

fn hydrate_view(
    view: post_views::Model,
    users_by_id: &std::collections::HashMap<Uuid, UserResponse>,
) -> PostViewResponse {
    let user_response = view
        .user_id
        .and_then(|user_id| users_by_id.get(&user_id).cloned());
    PostViewResponse::from_entity(view, user_response)
}

async fn load_view_user_map(
    db: &DatabaseConnection,
    views: &[post_views::Model],
) -> Result<std::collections::HashMap<Uuid, UserResponse>, DbErr> {
    user_hydration::load_user_response_map(
        db,
        views.iter().filter_map(|view| view.user_id),
        crate::models::user::UserView::General,
    )
    .await
}

pub async fn record_view(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Option<Uuid>,
    ip_address: Option<String>,
    user_agent: Option<String>,
) -> Result<(), PostViewError> {
    if !post_exists(db, post_id).await? {
        return Err(PostViewError::PostNotFound);
    }

    if let Some(user_id) = user_id
        && has_user_viewed(db, post_id, user_id).await?
    {
        return Ok(());
    }

    let now = Utc::now();
    post_views::ActiveModel {
        id: Set(Uuid::new_v4()),
        post_id: Set(post_id),
        user_id: Set(user_id),
        ip_address: Set(ip_address.filter(|value| !value.trim().is_empty())),
        user_agent: Set(user_agent.filter(|value| !value.trim().is_empty())),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
        deleted_at: Set(None),
    }
    .insert(db)
    .await?;

    Ok(())
}

pub async fn get_views_by_post_id(
    db: &DatabaseConnection,
    post_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<PostViewResponse>, i64), PostViewError> {
    if !post_exists(db, post_id).await? {
        return Err(PostViewError::PostNotFound);
    }

    let query = post_views::Entity::find()
        .filter(post_views::Column::PostId.eq(post_id))
        .filter(post_views::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let view_models = query
        .order_by_desc(post_views::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let users_by_id = load_view_user_map(db, &view_models).await?;
    let views = view_models
        .into_iter()
        .map(|view| hydrate_view(view, &users_by_id))
        .collect();

    Ok((views, total))
}

pub async fn get_view_stats(
    db: &DatabaseConnection,
    post_id: Uuid,
) -> Result<PostViewStats, PostViewError> {
    if !post_exists(db, post_id).await? {
        return Err(PostViewError::PostNotFound);
    }

    let views = post_views::Entity::find()
        .filter(post_views::Column::PostId.eq(post_id))
        .filter(post_views::Column::DeletedAt.is_null())
        .all(db)
        .await?;

    let total_views = views.len() as i64;
    let authenticated_views = views.iter().filter(|view| view.user_id.is_some()).count() as i64;
    let anonymous_views = total_views - authenticated_views;
    let unique_views = views
        .iter()
        .filter_map(|view| view.user_id)
        .collect::<HashSet<_>>()
        .len() as i64;

    Ok(PostViewStats {
        post_id,
        total_views,
        unique_views,
        anonymous_views,
        authenticated_views,
    })
}

pub async fn has_user_viewed_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<ViewStatusResponse, PostViewError> {
    if !post_exists(db, post_id).await? {
        return Err(PostViewError::PostNotFound);
    }

    Ok(ViewStatusResponse {
        has_viewed: has_user_viewed(db, post_id, user_id).await?,
    })
}

pub async fn get_my_posts_analytics(
    db: &DatabaseConnection,
    user_id: Uuid,
    start_date: Option<chrono::NaiveDate>,
    end_date: Option<chrono::NaiveDate>,
) -> Result<MyPostsAnalyticsResponse, DbErr> {
    let mut query = posts::Entity::find()
        .filter(posts::Column::CreatedBy.eq(user_id))
        .filter(posts::Column::DeletedAt.is_null());

    if let Some(start_date) = start_date {
        let start = start_date
            .and_hms_opt(0, 0, 0)
            .expect("valid start of day")
            .and_utc();
        query = query.filter(posts::Column::CreatedAt.gte(start));
    }
    if let Some(end_date) = end_date {
        let end = end_date
            .and_hms_opt(23, 59, 59)
            .expect("valid end of day")
            .and_utc();
        query = query.filter(posts::Column::CreatedAt.lte(end));
    }

    let post_models = query.all(db).await?;
    Ok(MyPostsAnalyticsResponse {
        total_posts: post_models.len() as i64,
        total_views: post_models
            .iter()
            .map(|post| post.view_count.unwrap_or(0))
            .sum(),
        total_likes: post_models
            .iter()
            .map(|post| post.like_count.unwrap_or(0))
            .sum(),
        total_bookmarks: post_models
            .iter()
            .map(|post| post.bookmark_count.unwrap_or(0))
            .sum(),
    })
}

pub async fn get_my_posts_likes_by_month(
    db: &DatabaseConnection,
    user_id: Uuid,
    months: i64,
) -> Result<Vec<MyPostsLikesByMonthResponse>, DbErr> {
    #[derive(FromQueryResult)]
    struct Row {
        month: String,
        total_likes: i64,
    }

    let rows: Vec<Row> = Row::find_by_statement(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        r#"
        SELECT to_char(date_trunc('month', pl.created_at), 'YYYY-MM') AS month,
               COUNT(*)::bigint AS total_likes
        FROM post_likes pl
        INNER JOIN posts p ON p.id = pl.post_id
        WHERE p.created_by = $1
          AND p.deleted_at IS NULL
          AND pl.deleted_at IS NULL
          AND pl.created_at >= date_trunc('month', now()) - (($2::int - 1) * interval '1 month')
        GROUP BY 1
        ORDER BY 1
        "#,
        vec![user_id.into(), months.into()],
    ))
    .all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| MyPostsLikesByMonthResponse {
            month: row.month,
            total_likes: row.total_likes,
        })
        .collect())
}
