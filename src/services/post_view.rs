use crate::entities::{post_views, posts};
use crate::models::post_view::{
    MyPostPerformance, MyPostsAnalyticsResponse, MyPostsAnalyticsSummary, MyPostsLikesByMonthPoint,
    MyPostsLikesByMonthResponse, MyPostsViewTrendPoint, PostViewResponse, PostViewStats,
    ViewStatusResponse,
};
use chrono::{Datelike, NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement,
};
use std::collections::{HashMap, HashSet};
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
        id: Set(Uuid::now_v7()),
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
    let views = query
        .order_by_desc(post_views::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?
        .into_iter()
        .map(PostViewResponse::from_entity)
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

/// Mirrors echobackend's `postViewService.GetMyPostsAnalytics`:
/// `{summary, view_trend, top_posts}` over a date window (default: the last
/// 30 days, swapped when reversed).
pub async fn get_my_posts_analytics(
    db: &DatabaseConnection,
    user_id: Uuid,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> Result<MyPostsAnalyticsResponse, DbErr> {
    let today = Utc::now().date_naive();
    let mut start = start_date.unwrap_or_else(|| today - chrono::Duration::days(30));
    let mut end = end_date.unwrap_or(today);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }

    #[derive(FromQueryResult)]
    struct SummaryRow {
        total_posts: i64,
        published_posts: i64,
        total_views: i64,
        total_likes: i64,
    }

    // Mirrors echobackend's unscoped `Table("posts")` query (soft-deleted
    // posts included).
    let summary_row = SummaryRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT COUNT(*)::bigint AS total_posts,
               COUNT(*) FILTER (WHERE published = true)::bigint AS published_posts,
               COALESCE(SUM(view_count), 0)::bigint AS total_views,
               COALESCE(SUM(like_count), 0)::bigint AS total_likes
        FROM posts
        WHERE created_by = $1
        "#,
        vec![user_id.into()],
    ))
    .one(db)
    .await?
    .unwrap_or(SummaryRow {
        total_posts: 0,
        published_posts: 0,
        total_views: 0,
        total_likes: 0,
    });

    #[derive(FromQueryResult)]
    struct TopPostRow {
        id: Uuid,
        title: String,
        slug: String,
        view_count: Option<i64>,
        like_count: Option<i64>,
    }

    let top_rows = TopPostRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT id, title, slug, view_count, like_count
        FROM posts
        WHERE created_by = $1
        ORDER BY view_count DESC, like_count DESC, created_at DESC
        LIMIT 5
        "#,
        vec![user_id.into()],
    ))
    .all(db)
    .await?;

    #[derive(FromQueryResult)]
    struct TrendRow {
        date: String,
        count: i64,
    }

    let trend_rows = TrendRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT to_char(DATE(pv.created_at), 'YYYY-MM-DD') AS date,
               COUNT(*)::bigint AS count
        FROM post_views pv
        INNER JOIN posts p ON p.id = pv.post_id AND p.deleted_at IS NULL
        WHERE p.created_by = $1
          AND pv.deleted_at IS NULL
          AND DATE(pv.created_at) >= $2
          AND DATE(pv.created_at) <= $3
        GROUP BY DATE(pv.created_at)
        ORDER BY DATE(pv.created_at) ASC
        "#,
        vec![user_id.into(), start.into(), end.into()],
    ))
    .all(db)
    .await?;

    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }

    let before_row = CountRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT COUNT(*)::bigint AS count
        FROM post_views pv
        INNER JOIN posts p ON p.id = pv.post_id AND p.deleted_at IS NULL
        WHERE p.created_by = $1
          AND pv.deleted_at IS NULL
          AND DATE(pv.created_at) < $2
        "#,
        vec![user_id.into(), start.into()],
    ))
    .one(db)
    .await?;

    let views_by_date: HashMap<String, i64> = trend_rows
        .into_iter()
        .map(|row| (row.date, row.count))
        .collect();

    let mut cumulative = before_row.map(|row| row.count).unwrap_or(0);
    let mut view_trend = Vec::new();
    let mut day = start;
    while day <= end {
        let date = day.format("%Y-%m-%d").to_string();
        let views = views_by_date.get(&date).copied().unwrap_or(0);
        cumulative += views;
        view_trend.push(MyPostsViewTrendPoint {
            date,
            views,
            cumulative_views: cumulative,
        });
        day += chrono::Duration::days(1);
    }

    Ok(MyPostsAnalyticsResponse {
        summary: MyPostsAnalyticsSummary {
            total_posts: summary_row.total_posts,
            published_posts: summary_row.published_posts,
            total_views: summary_row.total_views,
            total_likes: summary_row.total_likes,
        },
        view_trend,
        top_posts: top_rows
            .into_iter()
            .map(|row| MyPostPerformance {
                id: row.id,
                title: row.title,
                slug: row.slug,
                view_count: row.view_count.unwrap_or(0),
                like_count: row.like_count.unwrap_or(0),
            })
            .collect(),
    })
}

/// Mirrors echobackend's `postViewService.GetMyPostsLikesByMonth`: a
/// zero-filled monthly series with the running `months` window and `total`.
pub async fn get_my_posts_likes_by_month(
    db: &DatabaseConnection,
    user_id: Uuid,
    months: i64,
) -> Result<MyPostsLikesByMonthResponse, DbErr> {
    // echobackend clamps leniently: anything outside 1..=24 falls back to 12.
    let months = if (1..=24).contains(&months) {
        months
    } else {
        12
    };

    let now = Utc::now().date_naive();
    let end_month = now.with_day(1).expect("day 1 is always valid");
    let start_month = shift_months(end_month, -(months as i32 - 1));
    let end_exclusive = shift_months(end_month, 1);

    #[derive(FromQueryResult)]
    struct Row {
        month: String,
        count: i64,
    }

    let rows: Vec<Row> = Row::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT to_char(date_trunc('month', pl.created_at), 'YYYY-MM') AS month,
               COUNT(*)::bigint AS count
        FROM post_likes pl
        INNER JOIN posts p ON p.id = pl.post_id AND p.deleted_at IS NULL
        WHERE p.created_by = $1
          AND pl.created_at >= $2
          AND pl.created_at < $3
        GROUP BY date_trunc('month', pl.created_at)
        ORDER BY date_trunc('month', pl.created_at) ASC
        "#,
        vec![user_id.into(), start_month.into(), end_exclusive.into()],
    ))
    .all(db)
    .await?;

    let likes_by_month: HashMap<String, i64> =
        rows.into_iter().map(|row| (row.month, row.count)).collect();

    let mut total = 0;
    let mut series = Vec::with_capacity(months as usize);
    for i in 0..months as i32 {
        let month = shift_months(start_month, i).format("%Y-%m").to_string();
        let likes = likes_by_month.get(&month).copied().unwrap_or(0);
        total += likes;
        series.push(MyPostsLikesByMonthPoint { month, likes });
    }

    Ok(MyPostsLikesByMonthResponse {
        months,
        series,
        total,
    })
}

/// Shifts a first-of-month date by `delta` calendar months.
fn shift_months(first_of_month: NaiveDate, delta: i32) -> NaiveDate {
    let total = first_of_month.year() * 12 + (first_of_month.month() as i32 - 1) + delta;
    NaiveDate::from_ymd_opt(total.div_euclid(12), (total.rem_euclid(12) + 1) as u32, 1)
        .expect("a valid month always exists")
}
