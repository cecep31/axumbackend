use crate::dto::post::{MyPostsAnalyticsQuery, MyPostsLikesByMonthQuery};
use crate::dto::stats::StatsQuery;
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::VQuery;
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::stats::{EngagementMetricsResponse, PostStatsResponse};
use crate::services;
use axum::{Json, extract::State};

pub async fn get_my_posts_analytics(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<MyPostsAnalyticsQuery>,
) -> Result<Json<ApiResponse<crate::models::post_view::MyPostsAnalyticsResponse>>, AppError> {
    // echobackend parses dates leniently: unparseable values fall back to the
    // default window (last 30 days).
    let start_date = query
        .start_date
        .as_deref()
        .and_then(|value| chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok());
    let end_date = query
        .end_date
        .as_deref()
        .and_then(|value| chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok());

    // echobackend `respondError(c, "Failed to get post analytics", err)`: a
    // span over 366 days is 400 with the domain text as `error`.
    let analytics =
        services::post_view::get_my_posts_analytics(&pool, auth_user.id, start_date, end_date)
            .await
            .map_err(|e| AppError::from(e).context("Failed to get post analytics"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved post analytics",
        analytics,
    )))
}

pub async fn get_my_posts_likes_by_month(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<MyPostsLikesByMonthQuery>,
) -> Result<Json<ApiResponse<crate::models::post_view::MyPostsLikesByMonthResponse>>, AppError> {
    let data = services::post_view::get_my_posts_likes_by_month(
        &pool,
        auth_user.id,
        query.months.unwrap_or(12),
    )
    .await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved likes by month",
        data,
    )))
}

pub async fn get_post_stats(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VQuery(query): VQuery<StatsQuery>,
) -> Result<Json<ApiResponse<PostStatsResponse>>, AppError> {
    let stats =
        services::stats::post_stats(&pool, query.date_range(), query.limit(), query.tag_id())
            .await?;
    Ok(Json(ApiResponse::success_with_message(
        "Post stats fetched successfully",
        stats,
    )))
}

pub async fn get_post_engagement(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VQuery(query): VQuery<StatsQuery>,
) -> Result<Json<ApiResponse<EngagementMetricsResponse>>, AppError> {
    let metrics = services::stats::engagement(&pool, query.date_range()).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Engagement metrics fetched successfully",
        metrics,
    )))
}
