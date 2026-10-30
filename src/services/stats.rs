use crate::entities::{post_comments, posts, users};
use crate::models::stats::*;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    FromQueryResult, PaginatorTrait, QueryFilter, Statement, Value,
};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct DateRange<'a> {
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
}

/// `YYYY-MM-DD`, trimmed. Empty, whitespace-only or unparsable values are
/// absent: echobackend skips `""` filters, and a value Postgres would reject
/// is never spliced into SQL (dates are only ever bound parameters).
fn parse_date(value: Option<&str>) -> Option<NaiveDate> {
    value.and_then(|value| NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok())
}

/// UTC midnight of `date`, like Go's `time.Parse("2006-01-02", ..)`.
fn midnight_utc(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0)
        .expect("midnight is a valid time")
        .and_utc()
}

/// `" AND DATE(created_at) >= $n AND DATE(created_at) <= $m"` for the parsed
/// bounds of `range`, pushing the bound values onto `values`.
fn date_bounds_clause(range: DateRange<'_>, values: &mut Vec<Value>) -> String {
    let mut clause = String::new();
    if let Some(start) = parse_date(range.start_date) {
        values.push(start.into());
        clause.push_str(&format!(" AND DATE(created_at) >= ${}", values.len()));
    }
    if let Some(end) = parse_date(range.end_date) {
        values.push(end.into());
        clause.push_str(&format!(" AND DATE(created_at) <= ${}", values.len()));
    }
    clause
}

/// Bounds of the engagement "previous period" (echobackend
/// `GetEngagementMetrics`), on full timestamps: `now-60d..now-30d` by
/// default; with a parsable `start_date`, the window of the same length
/// right before it (`end` is `end_date` midnight, or `now`).
fn previous_period(range: DateRange<'_>, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    match parse_date(range.start_date).map(midnight_utc) {
        Some(start) => {
            let end = parse_date(range.end_date).map(midnight_utc).unwrap_or(now);
            let duration = end - start;
            (start - duration, start)
        }
        None => (now - Duration::days(60), now - Duration::days(30)),
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> Result<i64, DbErr> {
    scalar_i64_with(db, sql, Vec::new()).await
}

async fn scalar_i64_with(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Result<i64, DbErr> {
    let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?
    else {
        return Ok(0);
    };
    row.try_get("", "value")
}

pub async fn user_stats(
    db: &DatabaseConnection,
    range: DateRange<'_>,
    limit: i64,
) -> Result<UserStatsResponse, DbErr> {
    #[derive(FromQueryResult)]
    struct ContributorRow {
        id: Uuid,
        username: Option<String>,
        first_name: Option<String>,
        last_name: Option<String>,
        post_count: i64,
        total_views: i64,
        total_likes: i64,
    }

    // echobackend counts through `Model(&model.User{})`, whose soft-delete
    // scope adds `deleted_at IS NULL` to every user count.
    let total_users = users::Entity::find()
        .filter(users::Column::DeletedAt.is_null())
        .count(db)
        .await? as i64;
    let mut values = Vec::new();
    let bounds = date_bounds_clause(range, &mut values);
    let new_users_this_period = scalar_i64_with(
        db,
        &format!("SELECT COUNT(*)::bigint AS value FROM users WHERE deleted_at IS NULL{bounds}"),
        values,
    )
    .await?;
    let active_users = scalar_i64(
        db,
        "SELECT COUNT(DISTINCT user_id)::bigint AS value FROM post_views WHERE user_id IS NOT NULL AND deleted_at IS NULL AND created_at >= NOW() - INTERVAL '30 days'",
    )
    .await?;
    let today = Utc::now().date_naive();
    let week_ago = today - Duration::days(7);
    let new_users_today = scalar_i64_with(
        db,
        "SELECT COUNT(*)::bigint AS value FROM users WHERE deleted_at IS NULL AND DATE(created_at) >= $1",
        vec![today.into()],
    )
    .await?;
    let active_users_this_week = scalar_i64_with(
        db,
        "SELECT COUNT(DISTINCT user_id)::bigint AS value FROM post_views WHERE user_id IS NOT NULL AND deleted_at IS NULL AND DATE(created_at) >= $1",
        vec![week_ago.into()],
    )
    .await?;

    let rows = ContributorRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT users.id, users.username, users.first_name, users.last_name, COUNT(posts.id)::bigint AS post_count, COALESCE(SUM(posts.view_count), 0)::bigint AS total_views, COALESCE(SUM(posts.like_count), 0)::bigint AS total_likes FROM users LEFT JOIN posts ON users.id = posts.created_by WHERE users.deleted_at IS NULL GROUP BY users.id, users.username, users.first_name, users.last_name ORDER BY COUNT(posts.id) DESC LIMIT $1",
        vec![limit.into()],
    ))
    .all(db)
    .await?;

    let top_contributors = rows
        .into_iter()
        .map(|row| TopContributor {
            id: row.id,
            username: row.username,
            first_name: row.first_name,
            last_name: row.last_name,
            post_count: row.post_count,
            total_views: row.total_views,
            total_likes: row.total_likes,
        })
        .collect();

    Ok(UserStatsResponse {
        total_users,
        new_users_this_period,
        active_users,
        new_users_today,
        active_users_this_week,
        top_contributors,
        growth_trend: user_growth_trend(db, range).await?,
    })
}

async fn user_growth_trend(
    db: &DatabaseConnection,
    range: DateRange<'_>,
) -> Result<Vec<UserGrowthData>, DbErr> {
    #[derive(FromQueryResult)]
    struct DayCount {
        date: String,
        count: i64,
    }

    // echobackend computes `start`/`end` independently: `start` defaults to
    // `now - 30 days` regardless of whether `end_date` was supplied.
    let end = parse_date(range.end_date).unwrap_or_else(|| Utc::now().date_naive());
    let start = parse_date(range.start_date)
        .unwrap_or_else(|| Utc::now().date_naive() - Duration::days(30));
    // Both the per-day counts and the cumulative baseline skip soft-deleted
    // users (echobackend `GetUserGrowthTrendData`).
    let rows = DayCount::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT DATE(created_at)::text AS date, COUNT(*)::bigint AS count FROM users WHERE deleted_at IS NULL AND DATE(created_at) >= $1 AND DATE(created_at) <= $2 GROUP BY DATE(created_at) ORDER BY DATE(created_at) ASC",
        vec![start.into(), end.into()],
    ))
    .all(db)
    .await?;
    let mut counts = std::collections::HashMap::new();
    for row in rows {
        counts.insert(row.date, row.count);
    }
    let mut cumulative = scalar_i64_with(
        db,
        "SELECT COUNT(*)::bigint AS value FROM users WHERE deleted_at IS NULL AND DATE(created_at) < $1",
        vec![start.into()],
    )
    .await?;
    let mut result = Vec::new();
    let mut day = start;
    while day <= end {
        let date = day.to_string();
        let new_users = *counts.get(&date).unwrap_or(&0);
        cumulative += new_users;
        result.push(UserGrowthData {
            date,
            new_users,
            cumulative_users: cumulative,
        });
        day += Duration::days(1);
    }
    Ok(result)
}

pub async fn engagement(
    db: &DatabaseConnection,
    range: DateRange<'_>,
) -> Result<EngagementMetricsResponse, DbErr> {
    // echobackend `GetEngagementCounts`: `created_at >= startDate` and, when
    // `endDate` parses, `created_at <= endDate + 24h` (the whole end day).
    // Both bounds are bound parameters, never spliced into the SQL.
    let mut values: Vec<Value> = Vec::new();
    let mut bounds = String::new();
    if let Some(start) = parse_date(range.start_date) {
        values.push(start.into());
        bounds.push_str(&format!(" AND created_at >= ${}", values.len()));
    }
    if let Some(end) = parse_date(range.end_date) {
        values.push((midnight_utc(end) + Duration::hours(24)).into());
        bounds.push_str(&format!(" AND created_at <= ${}", values.len()));
    }
    let current_likes = scalar_i64_with(
        db,
        &format!("SELECT COUNT(*)::bigint AS value FROM post_likes WHERE 1=1{bounds}"),
        values.clone(),
    )
    .await?;
    // `Model(&model.PostComment{})` carries the soft-delete scope.
    let current_comments = scalar_i64_with(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM post_comments WHERE deleted_at IS NULL{bounds}"
        ),
        values,
    )
    .await?;
    let total_posts = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .count(db)
        .await? as i64;
    let total_views = scalar_i64(
        db,
        "SELECT COALESCE(SUM(view_count), 0)::bigint AS value FROM posts WHERE published = true",
    )
    .await?;
    let (prev_start, prev_end) = previous_period(range, Utc::now());
    let previous_likes = scalar_i64_with(
        db,
        "SELECT COUNT(*)::bigint AS value FROM post_likes WHERE created_at >= $1 AND created_at <= $2",
        vec![prev_start.into(), prev_end.into()],
    )
    .await?;
    let change_percent = if previous_likes > 0 {
        round2(((current_likes - previous_likes) as f64 / previous_likes as f64) * 100.0)
    } else {
        0.0
    };

    Ok(EngagementMetricsResponse {
        total_engagements: current_likes + current_comments,
        avg_likes_per_post: if total_posts > 0 {
            round2(current_likes as f64 / total_posts as f64)
        } else {
            0.0
        },
        avg_comments_per_post: if total_posts > 0 {
            round2(current_comments as f64 / total_posts as f64)
        } else {
            0.0
        },
        avg_views_per_post: if total_posts > 0 {
            round2(total_views as f64 / total_posts as f64)
        } else {
            0.0
        },
        period_comparison: PeriodComparison {
            current: current_likes,
            previous: previous_likes,
            change_percent,
        },
    })
}

pub async fn post_stats(
    db: &DatabaseConnection,
    range: DateRange<'_>,
    limit: i64,
    tag_id: Option<i32>,
) -> Result<PostStatsResponse, DbErr> {
    #[derive(FromQueryResult)]
    struct PostRow {
        id: Uuid,
        title: Option<String>,
        slug: Option<String>,
        views: i64,
        likes: i64,
        comments: i64,
        author_id: Uuid,
        author_username: Option<String>,
        author_first_name: Option<String>,
        author_last_name: Option<String>,
        created_at: Option<String>,
    }

    let total_posts = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .count(db)
        .await? as i64;
    let mut values = Vec::new();
    let bounds = date_bounds_clause(range, &mut values);
    let new_posts_this_period = scalar_i64_with(
        db,
        &format!("SELECT COUNT(*)::bigint AS value FROM posts WHERE published = true{bounds}"),
        values,
    )
    .await?;
    let total_views = scalar_i64(
        db,
        "SELECT COALESCE(SUM(view_count), 0)::bigint AS value FROM posts WHERE published = true",
    )
    .await?;
    let total_likes = scalar_i64(
        db,
        "SELECT COALESCE(SUM(like_count), 0)::bigint AS value FROM posts WHERE published = true",
    )
    .await?;
    // `Model(&model.PostComment{})`: soft-deleted comments are not counted.
    let total_comments = post_comments::Entity::find()
        .filter(post_comments::Column::DeletedAt.is_null())
        .count(db)
        .await? as i64;
    let today = Utc::now().date_naive();
    let new_posts_today = scalar_i64_with(
        db,
        "SELECT COUNT(*)::bigint AS value FROM posts WHERE published = true AND DATE(created_at) >= $1",
        vec![today.into()],
    )
    .await?;
    let mut values: Vec<Value> = vec![limit.into()];
    let (tag_join, tag_where) = match tag_id {
        Some(id) => {
            values.push(id.into());
            (
                " INNER JOIN posts_to_tags filter_tags ON posts.id = filter_tags.post_id",
                " AND filter_tags.tag_id = $2",
            )
        }
        None => ("", ""),
    };
    // echobackend `GetTopPosts`: authors must not be soft-deleted; the
    // per-post comment count is unscoped (`Table("post_comments")`), and
    // `created_at` is RFC 3339 like Go's `time.Time` scanned into a string.
    let rows = PostRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!(
            "SELECT posts.id, posts.title, posts.slug, COALESCE(posts.view_count, 0)::bigint AS views, COALESCE(posts.like_count, 0)::bigint AS likes, COUNT(post_comments.id)::bigint AS comments, users.id AS author_id, users.username AS author_username, users.first_name AS author_first_name, users.last_name AS author_last_name, to_char(posts.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at FROM posts INNER JOIN users ON posts.created_by = users.id AND users.deleted_at IS NULL LEFT JOIN post_comments ON post_comments.post_id = posts.id{tag_join} WHERE posts.published = true{tag_where} GROUP BY posts.id, posts.title, posts.slug, posts.view_count, posts.like_count, users.id, users.username, users.first_name, users.last_name, posts.created_at ORDER BY COALESCE(posts.view_count, 0) DESC LIMIT $1"
        ),
        values,
    ))
    .all(db)
    .await?;
    let top_posts = rows
        .into_iter()
        .map(|row| {
            let engagement_rate = if row.views > 0 {
                round2(((row.likes + row.comments) as f64 / row.views as f64) * 100.0)
            } else {
                0.0
            };
            PostPerformanceData {
                id: row.id,
                title: row.title,
                slug: row.slug,
                views: row.views,
                likes: row.likes,
                comments: row.comments,
                engagement_rate,
                author: PostPerformanceAuthor {
                    id: row.author_id,
                    username: row.author_username,
                    first_name: row.author_first_name,
                    last_name: row.author_last_name,
                },
                created_at: row.created_at,
            }
        })
        .collect();
    let avg_engagement_rate = if total_views > 0 {
        round2(((total_likes + total_comments) as f64 / total_views as f64) * 100.0)
    } else {
        0.0
    };

    Ok(PostStatsResponse {
        total_posts,
        new_posts_this_period,
        total_views,
        total_likes,
        total_comments,
        new_posts_today,
        avg_engagement_rate,
        top_posts,
    })
}

pub async fn tag_stats(db: &DatabaseConnection, limit: i64) -> Result<Vec<TagPerformance>, DbErr> {
    #[derive(FromQueryResult)]
    struct TagRow {
        id: i32,
        name: String,
        post_count: i64,
        total_views: i64,
        total_likes: i64,
    }
    let rows = TagRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT tags.id, tags.name, COUNT(posts_to_tags.post_id)::bigint AS post_count, COALESCE(SUM(posts.view_count), 0)::bigint AS total_views, COALESCE(SUM(posts.like_count), 0)::bigint AS total_likes FROM tags INNER JOIN posts_to_tags ON tags.id = posts_to_tags.tag_id INNER JOIN posts ON posts_to_tags.post_id = posts.id WHERE posts.published = true GROUP BY tags.id, tags.name ORDER BY COUNT(posts_to_tags.post_id) DESC LIMIT $1",
        vec![limit.into()],
    ))
    .all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| TagPerformance {
            id: row.id,
            name: row.name,
            post_count: row.post_count,
            total_views: row.total_views,
            total_likes: row.total_likes,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range<'a>(start: Option<&'a str>, end: Option<&'a str>) -> DateRange<'a> {
        DateRange {
            start_date: start,
            end_date: end,
        }
    }

    #[test]
    fn parse_date_treats_empty_and_garbage_as_absent() {
        assert_eq!(parse_date(None), None);
        assert_eq!(parse_date(Some("")), None);
        assert_eq!(parse_date(Some("   ")), None);
        assert_eq!(parse_date(Some("2026-01-01' OR 1=1 --")), None);
        assert_eq!(
            parse_date(Some(" 2026-01-02 ")),
            NaiveDate::from_ymd_opt(2026, 1, 2)
        );
    }

    #[test]
    fn date_bounds_are_bound_parameters() {
        let mut values = Vec::new();
        let clause = date_bounds_clause(
            range(Some("2026-01-01"), Some("x'); DROP TABLE users; --")),
            &mut values,
        );
        assert_eq!(clause, " AND DATE(created_at) >= $1");
        assert_eq!(values.len(), 1);

        let mut values = Vec::new();
        let clause = date_bounds_clause(range(Some(""), Some("2026-02-01")), &mut values);
        assert_eq!(clause, " AND DATE(created_at) <= $1");
        assert_eq!(values.len(), 1);

        let mut values = Vec::new();
        let clause = date_bounds_clause(range(Some("2026-01-01"), Some("2026-02-01")), &mut values);
        assert_eq!(
            clause,
            " AND DATE(created_at) >= $1 AND DATE(created_at) <= $2"
        );
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn previous_period_keeps_time_of_day() {
        let now = DateTime::parse_from_rfc3339("2026-10-30T15:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (start, end) = previous_period(range(None, None), now);
        assert_eq!(start, now - Duration::days(60));
        assert_eq!(end, now - Duration::days(30));

        // start parses, end defaults to now: duration carries the fraction.
        let (start, end) = previous_period(range(Some("2026-10-20"), Some("")), now);
        let start_midnight = midnight_utc(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap());
        assert_eq!(end, start_midnight);
        assert_eq!(start, start_midnight - (now - start_midnight));

        // both parse: whole days.
        let (start, end) = previous_period(range(Some("2026-10-20"), Some("2026-10-30")), now);
        assert_eq!(end, start_midnight);
        assert_eq!(start, start_midnight - Duration::days(10));
    }
}
