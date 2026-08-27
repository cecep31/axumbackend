use crate::entities::{post_comments, post_likes, post_views, posts, users};
use crate::models::report::*;
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    FromQueryResult, PaginatorTrait, QueryFilter, Statement,
};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct DateRange<'a> {
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
}

fn parse_date(value: Option<&str>) -> Option<NaiveDate> {
    value.and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> Result<i64, DbErr> {
    let Some(row) = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql.to_string()))
        .await?
    else {
        return Ok(0);
    };
    row.try_get("", "value")
}

pub async fn overview(db: &DatabaseConnection) -> Result<OverviewStatsResponse, DbErr> {
    let today = Utc::now().date_naive();
    let week_ago = today - Duration::days(7);

    let total_users = users::Entity::find().count(db).await? as i64;
    let total_posts = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .count(db)
        .await? as i64;
    let total_views = post_views::Entity::find().count(db).await? as i64;
    let total_likes = post_likes::Entity::find().count(db).await? as i64;
    let total_comments = post_comments::Entity::find().count(db).await? as i64;
    let new_users_today = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM users WHERE DATE(created_at) >= '{}'",
            today
        ),
    )
    .await?;
    let new_posts_today = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM posts WHERE published = true AND DATE(created_at) >= '{}'",
            today
        ),
    )
    .await?;
    let active_users_this_week = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(DISTINCT user_id)::bigint AS value FROM post_views WHERE user_id IS NOT NULL AND DATE(created_at) >= '{}'",
            week_ago
        ),
    )
    .await?;

    Ok(OverviewStatsResponse {
        total_users,
        total_posts,
        total_views,
        total_likes,
        total_comments,
        new_users_today,
        new_posts_today,
        active_users_this_week,
    })
}

pub async fn user_report(
    db: &DatabaseConnection,
    range: DateRange<'_>,
    limit: i64,
) -> Result<UserReportResponse, DbErr> {
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

    let total_users = users::Entity::find().count(db).await? as i64;
    let start_clause = range
        .start_date
        .map(|v| format!(" AND DATE(created_at) >= '{}'", v))
        .unwrap_or_default();
    let end_clause = range
        .end_date
        .map(|v| format!(" AND DATE(created_at) <= '{}'", v))
        .unwrap_or_default();
    let new_users_this_period = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM users WHERE 1=1{}{}",
            start_clause, end_clause
        ),
    )
    .await?;
    let active_users = scalar_i64(
        db,
        "SELECT COUNT(DISTINCT user_id)::bigint AS value FROM post_views WHERE user_id IS NOT NULL AND created_at >= NOW() - INTERVAL '30 days'",
    )
    .await?;

    let rows = ContributorRow::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT users.id, users.username, users.first_name, users.last_name, COUNT(posts.id)::bigint AS post_count, COALESCE(SUM(posts.view_count), 0)::bigint AS total_views, COALESCE(SUM(posts.like_count), 0)::bigint AS total_likes FROM users LEFT JOIN posts ON users.id = posts.created_by AND posts.deleted_at IS NULL GROUP BY users.id, users.username, users.first_name, users.last_name ORDER BY COUNT(posts.id) DESC LIMIT {}",
            limit
        ),
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

    Ok(UserReportResponse {
        total_users,
        new_users_this_period,
        active_users,
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
    let rows = DayCount::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT DATE(created_at)::text AS date, COUNT(*)::bigint AS count FROM users WHERE DATE(created_at) >= '{}' AND DATE(created_at) <= '{}' GROUP BY DATE(created_at) ORDER BY DATE(created_at) ASC",
            start, end
        ),
    ))
    .all(db)
    .await?;
    let mut counts = std::collections::HashMap::new();
    for row in rows {
        counts.insert(row.date, row.count);
    }
    let mut cumulative = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM users WHERE DATE(created_at) < '{}'",
            start
        ),
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
    let start_clause = range
        .start_date
        .map(|v| format!(" AND created_at >= '{}'", v))
        .unwrap_or_default();
    let end_clause = range
        .end_date
        .map(|v| format!(" AND created_at <= ('{}'::date + INTERVAL '1 day')", v))
        .unwrap_or_default();
    let current_likes = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM post_likes WHERE 1=1{}{}",
            start_clause, end_clause
        ),
    )
    .await?;
    let current_comments = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM post_comments WHERE 1=1{}{}",
            start_clause, end_clause
        ),
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
    // Mirrors echobackend's GetEngagementMetrics: when `start_date` is given,
    // the previous period is derived dynamically as the window immediately
    // preceding it with the same duration; otherwise it falls back to the
    // fixed now-60d..now-30d window.
    let (prev_start, prev_end) = match parse_date(range.start_date) {
        Some(start_date) => {
            let end_date = parse_date(range.end_date).unwrap_or_else(|| Utc::now().date_naive());
            let duration = end_date - start_date;
            (start_date - duration, start_date)
        }
        None => (
            Utc::now().date_naive() - Duration::days(60),
            Utc::now().date_naive() - Duration::days(30),
        ),
    };
    let previous_likes = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM post_likes WHERE created_at >= '{}' AND created_at <= '{}'",
            prev_start, prev_end
        ),
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

pub async fn post_report(
    db: &DatabaseConnection,
    range: DateRange<'_>,
    limit: i64,
    tag_id: Option<i32>,
) -> Result<PostReportResponse, DbErr> {
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
    #[derive(FromQueryResult)]
    struct TagRow {
        id: i32,
        name: String,
        post_count: i64,
        total_views: i64,
        total_likes: i64,
    }

    let start_clause = range
        .start_date
        .map(|v| format!(" AND DATE(created_at) >= '{}'", v))
        .unwrap_or_default();
    let end_clause = range
        .end_date
        .map(|v| format!(" AND DATE(created_at) <= '{}'", v))
        .unwrap_or_default();
    let total_posts = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .count(db)
        .await? as i64;
    let new_posts_this_period = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*)::bigint AS value FROM posts WHERE published = true{}{}",
            start_clause, end_clause
        ),
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
    let total_comments = post_comments::Entity::find().count(db).await? as i64;
    let tag_join = if tag_id.is_some() {
        " INNER JOIN posts_to_tags filter_tags ON posts.id = filter_tags.post_id"
    } else {
        ""
    };
    let tag_where = tag_id
        .map(|id| format!(" AND filter_tags.tag_id = {}", id))
        .unwrap_or_default();
    let rows = PostRow::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT posts.id, posts.title, posts.slug, COALESCE(posts.view_count, 0)::bigint AS views, COALESCE(posts.like_count, 0)::bigint AS likes, COUNT(post_comments.id)::bigint AS comments, users.id AS author_id, users.username AS author_username, users.first_name AS author_first_name, users.last_name AS author_last_name, posts.created_at::text AS created_at FROM posts INNER JOIN users ON posts.created_by = users.id LEFT JOIN post_comments ON post_comments.post_id = posts.id{} WHERE posts.published = true{} GROUP BY posts.id, posts.title, posts.slug, posts.view_count, posts.like_count, users.id, users.username, users.first_name, users.last_name, posts.created_at ORDER BY COALESCE(posts.view_count, 0) DESC LIMIT {}",
            tag_join, tag_where, limit
        ),
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
    let tag_rows = TagRow::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        "SELECT tags.id, tags.name, COUNT(ptt.post_id)::bigint AS post_count, COALESCE(SUM(p.view_count), 0)::bigint AS total_views, COALESCE(SUM(p.like_count), 0)::bigint AS total_likes FROM tags INNER JOIN posts_to_tags ptt ON tags.id = ptt.tag_id INNER JOIN posts p ON ptt.post_id = p.id WHERE p.published = true GROUP BY tags.id, tags.name ORDER BY COUNT(ptt.post_id) DESC LIMIT 10".to_string(),
    ))
    .all(db)
    .await?;
    let tag_performance = tag_rows
        .into_iter()
        .map(|row| TagPerformance {
            id: row.id,
            name: row.name,
            post_count: row.post_count,
            total_views: row.total_views,
            total_likes: row.total_likes,
        })
        .collect();
    let avg_engagement_rate = if total_views > 0 {
        round2(((total_likes + total_comments) as f64 / total_views as f64) * 100.0)
    } else {
        0.0
    };

    Ok(PostReportResponse {
        total_posts,
        new_posts_this_period,
        total_views,
        total_likes,
        total_comments,
        avg_engagement_rate,
        top_posts,
        tag_performance,
    })
}
