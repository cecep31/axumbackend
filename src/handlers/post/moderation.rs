use crate::dto::common::PostIdPath;
use crate::dto::post_report::{CreatePostReportRequest, ModeratePostRequest, ReportStatusQuery};
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VJson, VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::post_report::{
    ModerationActionResponse, PostReportGroup, SinglePostReportResponse,
};
use crate::services;
use crate::services::post_report::PostReportError;
use axum::{Json, extract::State};

impl From<PostReportError> for AppError {
    fn from(err: PostReportError) -> Self {
        match err {
            PostReportError::InvalidPostId | PostReportError::InvalidUserId => {
                AppError::BadRequest("Invalid ID format".to_string())
            }
            PostReportError::InvalidReportStatus => {
                AppError::BadRequest("Invalid report status".to_string())
            }
            PostReportError::CannotReportOwnPost => {
                AppError::BadRequest("Cannot report your own post".to_string())
            }
            PostReportError::PostNotFound => AppError::NotFound("Post not found".to_string()),
            PostReportError::NoPendingReports
            | PostReportError::PostAlreadyHidden
            | PostReportError::PostNotHidden => {
                AppError::Conflict("Moderation state conflict".to_string())
            }
            PostReportError::Db(e) => AppError::from(e),
        }
    }
}

pub async fn report_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<CreatePostReportRequest>,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<serde_json::Value>>), AppError> {
    if !crate::services::post_report::REASON_CHOICES.contains(&req.reason.as_str()) {
        return Err(AppError::BadRequest("Invalid report reason".to_string()));
    }
    services::post_report::report_post(&pool, params.id, auth_user.id, &req.reason, req.details)
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Report submitted",
            serde_json::Value::Null,
        )),
    ))
}

pub async fn list_reported_posts(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VQuery(query): VQuery<ReportStatusQuery>,
) -> Result<Json<ApiResponse<Vec<PostReportGroup>>>, AppError> {
    let status = query.status_or_default();
    let (groups, total) = services::post_report::list_reported_posts(
        &pool,
        &status,
        query.limit_or(20),
        query.offset_or(),
    )
    .await?;
    let limit = query.limit_or(20) as i64;
    let offset = query.offset_or() as i64;
    Ok(Json(ApiResponse::with_meta_message(
        "Reported posts fetched successfully",
        groups,
        total as i64,
        limit,
        offset,
    )))
}

pub async fn list_post_reports(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VPath(params): VPath<PostIdPath>,
    VQuery(query): VQuery<ReportStatusQuery>,
) -> Result<Json<ApiResponse<Vec<SinglePostReportResponse>>>, AppError> {
    let status = query.status.as_deref().unwrap_or("");
    let (reports, total) = services::post_report::list_post_reports(
        &pool,
        params.id,
        Some(status),
        query.limit_or(20),
        query.offset_or(),
    )
    .await?;
    Ok(Json(ApiResponse::with_meta_message(
        "Post reports fetched successfully",
        reports,
        total as i64,
        query.limit_or(20) as i64,
        query.offset_or() as i64,
    )))
}

pub async fn moderate_post(
    State(pool): State<DbPool>,
    admin: AdminUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<ModeratePostRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    if !crate::services::post_report::ACTION_CHOICES.contains(&req.action.as_str()) {
        return Err(AppError::BadRequest(
            "Invalid moderation action".to_string(),
        ));
    }
    services::post_report::moderate_post(&pool, params.id, admin.0.id, &req.action, req.note)
        .await?;
    Ok(Json(ApiResponse::success_with_message(
        "Moderation action applied",
        serde_json::Value::Null,
    )))
}

pub async fn list_moderation_actions(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VPath(params): VPath<PostIdPath>,
    VQuery(query): VQuery<ReportStatusQuery>,
) -> Result<Json<ApiResponse<Vec<ModerationActionResponse>>>, AppError> {
    let (actions, total) = services::post_report::list_moderation_actions(
        &pool,
        params.id,
        query.limit_or(20),
        query.offset_or(),
    )
    .await?;
    Ok(Json(ApiResponse::with_meta_message(
        "Moderation history fetched successfully",
        actions,
        total as i64,
        query.limit_or(20) as i64,
        query.offset_or() as i64,
    )))
}
