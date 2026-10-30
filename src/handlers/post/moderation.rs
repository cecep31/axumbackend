use crate::dto::post_report::{
    CreatePostReportRequest, ModeratePostRequest, ReportPostPath, ReportStatusQuery,
};
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VJsonOrDefault, VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::post_report::{
    ModerationActionResponse, PostReportGroup, SinglePostReportResponse,
};
use crate::services;
use crate::services::post_report::PostReportError;
use axum::{Json, extract::State};
use uuid::Uuid;

/// echobackend `PostReportHandler.respondError(c, op, err)`:
/// - 400 `{message: <err text>}` (no `error`) for malformed ids, an invalid
///   status and self-reports;
/// - 404 `{message: "Post not found", error: "post not found"}`;
/// - 409 `{message: op, error: <conflict text>}`;
/// - anything else is a 500 with `op` and no `error`.
fn report_err(op: &'static str) -> impl Fn(PostReportError) -> AppError {
    move |err| match err {
        PostReportError::InvalidPostId => AppError::bad_request("invalid post ID format", None),
        PostReportError::InvalidUserId => AppError::bad_request("invalid user ID format", None),
        PostReportError::InvalidReportStatus => {
            AppError::bad_request("invalid report status", None)
        }
        PostReportError::CannotReportOwnPost => {
            AppError::bad_request("cannot report your own post", None)
        }
        PostReportError::PostNotFound => {
            AppError::not_found("Post not found", Some("post not found".to_string()))
        }
        PostReportError::NoPendingReports => AppError::conflict(op, "post has no pending reports"),
        PostReportError::PostAlreadyHidden => AppError::conflict(op, "post is already hidden"),
        PostReportError::PostNotHidden => AppError::conflict(op, "post is not hidden"),
        PostReportError::Db(e) => AppError::internal(op, e),
    }
}

/// echobackend `validator.IsValidUUID(c.Param("id"))` in the service.
fn parse_post_id(raw: &str, op: &'static str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| report_err(op)(PostReportError::InvalidPostId))
}

pub async fn report_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<ReportPostPath>,
    VJsonOrDefault(req): VJsonOrDefault<CreatePostReportRequest>,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<serde_json::Value>>), AppError> {
    const OP: &str = "Failed to report post";
    let post_id = parse_post_id(&params.id, OP)?;
    services::post_report::report_post(&pool, post_id, auth_user.id, &req.reason, req.details)
        .await
        .map_err(report_err(OP))?;
    // A repeat report by the same user also answers 201, so the response
    // never reveals whether the post was already reported.
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::message("Report submitted")),
    ))
}

pub async fn list_reported_posts(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VQuery(query): VQuery<ReportStatusQuery>,
) -> Result<Json<ApiResponse<Vec<PostReportGroup>>>, AppError> {
    let status = query.status_or_default();
    let (limit, offset) = (query.limit_or(20), query.offset_or());
    let (groups, total) = services::post_report::list_reported_posts(&pool, &status, limit, offset)
        .await
        .map_err(report_err("Failed to fetch reported posts"))?;
    Ok(Json(ApiResponse::with_meta_message(
        "Reported posts fetched successfully",
        groups,
        total as i64,
        limit as i64,
        offset as i64,
    )))
}

pub async fn list_post_reports(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VPath(params): VPath<ReportPostPath>,
    VQuery(query): VQuery<ReportStatusQuery>,
) -> Result<Json<ApiResponse<Vec<SinglePostReportResponse>>>, AppError> {
    const OP: &str = "Failed to fetch post reports";
    let post_id = parse_post_id(&params.id, OP)?;
    let status = query.status.as_deref().unwrap_or("");
    let (limit, offset) = (query.limit_or(20), query.offset_or());
    let (reports, total) =
        services::post_report::list_post_reports(&pool, post_id, Some(status), limit, offset)
            .await
            .map_err(report_err(OP))?;
    Ok(Json(ApiResponse::with_meta_message(
        "Post reports fetched successfully",
        reports,
        total as i64,
        limit as i64,
        offset as i64,
    )))
}

pub async fn moderate_post(
    State(pool): State<DbPool>,
    admin: AdminUser,
    VPath(params): VPath<ReportPostPath>,
    VJsonOrDefault(req): VJsonOrDefault<ModeratePostRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    const OP: &str = "Failed to moderate post";
    let post_id = parse_post_id(&params.id, OP)?;
    services::post_report::moderate_post(&pool, post_id, admin.0.id, &req.action, req.note)
        .await
        .map_err(report_err(OP))?;
    Ok(Json(ApiResponse::message("Moderation action applied")))
}

pub async fn list_moderation_actions(
    State(pool): State<DbPool>,
    _admin: AdminUser,
    VPath(params): VPath<ReportPostPath>,
    VQuery(query): VQuery<ReportStatusQuery>,
) -> Result<Json<ApiResponse<Vec<ModerationActionResponse>>>, AppError> {
    const OP: &str = "Failed to fetch moderation history";
    let post_id = parse_post_id(&params.id, OP)?;
    let (limit, offset) = (query.limit_or(20), query.offset_or());
    let (actions, total) =
        services::post_report::list_moderation_actions(&pool, post_id, limit, offset)
            .await
            .map_err(report_err(OP))?;
    Ok(Json(ApiResponse::with_meta_message(
        "Moderation history fetched successfully",
        actions,
        total as i64,
        limit as i64,
        offset as i64,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    async fn body(err: AppError) -> (u16, serde_json::Value) {
        let response = err.into_response();
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn report_errors_match_echobackend() {
        let op = "Failed to moderate post";
        let (status, json) = body(report_err(op)(PostReportError::PostAlreadyHidden)).await;
        assert_eq!(status, 409);
        assert_eq!(json["message"], op);
        assert_eq!(json["error"], "post is already hidden");

        let (status, json) = body(report_err(op)(PostReportError::NoPendingReports)).await;
        assert_eq!(status, 409);
        assert_eq!(json["error"], "post has no pending reports");

        let (status, json) = body(report_err(op)(PostReportError::CannotReportOwnPost)).await;
        assert_eq!(status, 400);
        assert_eq!(json["message"], "cannot report your own post");
        assert!(json.get("error").is_none());

        let (status, json) = body(parse_post_id("nope", op).unwrap_err()).await;
        assert_eq!(status, 400);
        assert_eq!(json["message"], "invalid post ID format");
        assert!(json.get("error").is_none());

        let (status, json) = body(report_err(op)(PostReportError::PostNotFound)).await;
        assert_eq!(status, 404);
        assert_eq!(json["message"], "Post not found");
        assert_eq!(json["error"], "post not found");

        let (status, json) = body(report_err(op)(PostReportError::Db(sea_orm::DbErr::Custom(
            "boom".into(),
        ))))
        .await;
        assert_eq!(status, 500);
        assert_eq!(json["message"], op);
        assert!(json.get("error").is_none());
    }
}
