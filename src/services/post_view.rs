use crate::entities::post_views;
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
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug)]
pub enum PostViewError {
    Db(DbErr),
    PostNotFound,
    /// The requester is neither the post author nor a super admin
    /// (echobackend `ErrNotAuthor`).
    NotAuthor,
    /// echobackend `ErrDateRangeTooLarge`.
    DateRangeTooLarge,
}

/// echobackend `maxAnalyticsRange`: `view_trend` has one point per day, so
/// the analytics window is bounded.
const MAX_ANALYTICS_RANGE: chrono::Duration = chrono::Duration::days(366);

/// echobackend `GetMyPostsAnalytics` window, compared on full timestamps:
/// `start` is the parsed date (UTC midnight) or `now - 30d`, `end` the parsed
/// date or `now`; reversed bounds are swapped, and a span over 366 days is
/// rejected. Returns the `(start, end)` calendar days.
fn analytics_window(
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    now: chrono::DateTime<Utc>,
) -> Result<(NaiveDate, NaiveDate), PostViewError> {
    let midnight = |d: NaiveDate| d.and_hms_opt(0, 0, 0).expect("valid time").and_utc();
    let mut start = start_date
        .map(midnight)
        .unwrap_or(now - chrono::Duration::days(30));
    let mut end = end_date.map(midnight).unwrap_or(now);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    if end - start > MAX_ANALYTICS_RANGE {
        return Err(PostViewError::DateRangeTooLarge);
    }
    Ok((start.date_naive(), end.date_naive()))
}

impl From<DbErr> for PostViewError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

async fn post_exists(db: &DatabaseConnection, post_id: Uuid) -> Result<bool, DbErr> {
    // Mirrors echobackend GetPostByID + ensurePostInteractable: a hidden post,
    // or one whose author is soft-deleted, is reported as missing so
    // engagement endpoints do not confirm that it exists.
    crate::services::post::interactable_post_exists(db, post_id).await
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

/// TTL of echobackend's `post_view:<postID>:<userID>` "already viewed" marker.
const VIEWED_MARKER_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Records a view. Mirrors echobackend `RecordView`: for a logged-in user the
/// Redis marker `post_view:<post>:<user>` (JSON `true`, 24h) short-circuits
/// repeat views with no DB access; it is set once a view is found or stored.
pub async fn record_view(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Option<Uuid>,
    ip_address: Option<String>,
    user_agent: Option<String>,
) -> Result<(), PostViewError> {
    let post_key = post_id.to_string();
    let user_key = user_id.map(|id| id.to_string());
    let marker = user_key
        .as_deref()
        .map(|user| ["post_view", post_key.as_str(), user]);
    if let Some(marker) = &marker
        && crate::infra::cache::read_json::<bool>(marker).await == Some(true)
    {
        return Ok(());
    }

    if !post_exists(db, post_id).await? {
        return Err(PostViewError::PostNotFound);
    }

    if let Some(user_id) = user_id
        && has_user_viewed(db, post_id, user_id).await?
    {
        if let Some(marker) = &marker {
            crate::infra::cache::write_json(marker, &true, VIEWED_MARKER_TTL).await;
        }
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

    if let Some(marker) = &marker {
        crate::infra::cache::write_json(marker, &true, VIEWED_MARKER_TTL).await;
    }
    Ok(())
}

/// Lists the raw view records of a post. Mirrors echobackend
/// `GetViewsByPostID`: only the post author or a super admin (`is_admin`,
/// DB-verified by the caller) may see who viewed it; `limit` is clamped to
/// `1..=100` (default 10) and `offset` to `>= 0`.
pub async fn get_views_by_post_id(
    db: &DatabaseConnection,
    post_id: Uuid,
    requester_id: Uuid,
    is_admin: bool,
    limit: i64,
    offset: i64,
) -> Result<(Vec<PostViewResponse>, i64), PostViewError> {
    // echobackend GetPostByID: hidden posts are allowed, a soft-deleted
    // author's post is missing.
    let Some(post) = crate::services::post::find_active_post(db, post_id).await? else {
        return Err(PostViewError::PostNotFound);
    };
    if !is_admin && post.created_by != requester_id {
        return Err(PostViewError::NotAuthor);
    }
    let limit = if limit <= 0 { 10 } else { limit.min(100) };
    let offset = offset.max(0);

    let query = post_views::Entity::find()
        .filter(post_views::Column::PostId.eq(post_id))
        .filter(post_views::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let views = query
        .order_by_desc(post_views::Column::CreatedAt)
        .limit(limit as u64)
        .offset(offset as u64)
        .all(db)
        .await?
        .into_iter()
        .map(PostViewResponse::from_entity)
        .collect();

    Ok((views, total))
}

/// Like echobackend `GetViewStats`, the post itself is not looked up: a
/// missing or hidden post simply has zero views.
pub async fn get_view_stats(
    db: &DatabaseConnection,
    post_id: Uuid,
) -> Result<PostViewStats, PostViewError> {
    #[derive(FromQueryResult)]
    struct ViewStatsRow {
        total_views: i64,
        authenticated_views: i64,
        unique_views: i64,
    }

    let row = ViewStatsRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT
            COUNT(*)::bigint AS total_views,
            COUNT(user_id)::bigint AS authenticated_views,
            COUNT(DISTINCT user_id)::bigint AS unique_views
        FROM post_views
        WHERE post_id = $1 AND deleted_at IS NULL
        "#,
        vec![post_id.into()],
    ))
    .one(db)
    .await?
    .unwrap_or(ViewStatsRow {
        total_views: 0,
        authenticated_views: 0,
        unique_views: 0,
    });

    let anonymous_views = row.total_views - row.authenticated_views;

    Ok(PostViewStats {
        post_id,
        total_views: row.total_views,
        unique_views: row.unique_views,
        anonymous_views,
        authenticated_views: row.authenticated_views,
    })
}

/// echobackend `HasUserViewedPost`: no post lookup (hidden posts included).
pub async fn has_user_viewed_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<ViewStatusResponse, PostViewError> {
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
) -> Result<MyPostsAnalyticsResponse, PostViewError> {
    let (start, end) = analytics_window(start_date, end_date, Utc::now())?;

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
        INNER JOIN posts p ON p.id = pv.post_id
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
        INNER JOIN posts p ON p.id = pv.post_id
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
        INNER JOIN posts p ON p.id = pl.post_id
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

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn now() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn analytics_window_defaults_and_swaps() {
        assert_eq!(
            analytics_window(None, None, now()).unwrap(),
            (date(2026, 9, 30), date(2026, 10, 30))
        );
        assert_eq!(
            analytics_window(Some(date(2026, 10, 10)), Some(date(2026, 10, 1)), now()).unwrap(),
            (date(2026, 10, 1), date(2026, 10, 10))
        );
    }

    #[test]
    fn analytics_window_caps_at_366_days() {
        // exactly 366 days between midnights is allowed
        assert!(analytics_window(Some(date(2025, 1, 1)), Some(date(2026, 1, 2)), now()).is_ok());
        assert!(matches!(
            analytics_window(Some(date(2025, 1, 1)), Some(date(2026, 1, 3)), now()),
            Err(PostViewError::DateRangeTooLarge)
        ));
        // 366 days back plus the time of day of the default `now` end
        assert!(matches!(
            analytics_window(Some(date(2025, 10, 29)), None, now()),
            Err(PostViewError::DateRangeTooLarge)
        ));
    }
}
