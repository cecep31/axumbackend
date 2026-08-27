use crate::auth::AdminUser;
use crate::database::DbPool;
use crate::dto::report::{OverviewReport, ReportQuery, date_range};
use crate::error::AppError;
use crate::extract::VQuery;
use crate::models::report::{EngagementMetricsResponse, PostReportResponse, UserReportResponse};
use crate::response::ApiResponse;
use crate::services;
use axum::{Json, Router, extract::State, routing::get};

pub async fn get_overview(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<ReportQuery>,
) -> Result<Json<ApiResponse<OverviewReport>>, AppError> {
    let overview = services::report::overview(&pool).await?;
    let engagement = services::report::engagement(&pool, date_range(&query)).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Overview report fetched successfully",
        OverviewReport {
            overview,
            engagement,
        },
    )))
}

pub async fn get_users(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<ReportQuery>,
) -> Result<Json<ApiResponse<UserReportResponse>>, AppError> {
    let report = services::report::user_report(&pool, date_range(&query), query.limit()).await?;
    Ok(Json(ApiResponse::success_with_message(
        "User report fetched successfully",
        report,
    )))
}

pub async fn get_posts(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<ReportQuery>,
) -> Result<Json<ApiResponse<PostReportResponse>>, AppError> {
    let report =
        services::report::post_report(&pool, date_range(&query), query.limit(), query.tag_id())
            .await?;
    Ok(Json(ApiResponse::success_with_message(
        "Post report fetched successfully",
        report,
    )))
}

pub async fn get_engagement(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<ReportQuery>,
) -> Result<Json<ApiResponse<EngagementMetricsResponse>>, AppError> {
    let report = services::report::engagement(&pool, date_range(&query)).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Engagement metrics fetched successfully",
        report,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/reports/overview", get(get_overview))
        .route("/api/reports/users", get(get_users))
        .route("/api/reports/posts", get(get_posts))
        .route("/api/reports/engagement", get(get_engagement))
}
