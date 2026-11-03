use crate::entities::{post_moderation_actions, post_reports, posts, users};
use crate::models::post_report::{
    ModerationActionResponse, PostReportGroup, SinglePostReportResponse, UserBrief,
};
use crate::services::pagination::paginate;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    Statement, TransactionTrait,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const REASON_CHOICES: [&str; 6] = [
    "spam",
    "harassment",
    "hate",
    "nsfw",
    "misinformation",
    "other",
];
pub const ACTION_CHOICES: [&str; 3] = ["hide", "unhide", "dismiss"];
pub const STATUS_CHOICES: [&str; 3] = ["pending", "resolved", "dismissed"];

#[derive(Debug)]
pub enum PostReportError {
    Db(DbErr),
    InvalidPostId,
    InvalidUserId,
    InvalidReportStatus,
    CannotReportOwnPost,
    PostNotFound,
    NoPendingReports,
    PostAlreadyHidden,
    PostNotHidden,
}

impl From<DbErr> for PostReportError {
    fn from(err: DbErr) -> Self {
        PostReportError::Db(err)
    }
}

/// echobackend `Preload("Reporter"/"Admin", preloadUserBrief)` goes through
/// GORM's soft-delete scope, so a soft-deleted user yields no brief (and the
/// `omitempty` key is left out).
/// Batched in one `id IN (...)` query; no query when `ids` is empty.
async fn active_users_by_id(
    db: &DatabaseConnection,
    ids: impl IntoIterator<Item = Uuid>,
) -> Result<HashMap<Uuid, users::Model>, DbErr> {
    let ids: HashSet<Uuid> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(users::Entity::find()
        .filter(users::Column::Id.is_in(ids))
        .filter(users::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect())
}

fn brief_from_user(u: Option<users::Model>) -> Option<UserBrief> {
    u.map(|u| UserBrief {
        id: u.id,
        username: u.username,
        image: u.image,
    })
}

fn action_to_response(
    a: post_moderation_actions::Model,
    admin: Option<users::Model>,
) -> ModerationActionResponse {
    ModerationActionResponse {
        id: a.id,
        post_id: a.post_id,
        action: a.action,
        note: a.note,
        created_at: a.created_at.to_rfc3339(),
        admin: brief_from_user(admin),
    }
}

pub async fn report_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    reporter_id: Uuid,
    reason: &str,
    details: Option<String>,
) -> Result<(), PostReportError> {
    if !REASON_CHOICES.contains(&reason) {
        return Err(PostReportError::InvalidReportStatus);
    }
    // echobackend GetPostByID (author not soft-deleted) + ensurePostInteractable.
    let post = crate::services::post::find_active_post(db, post_id)
        .await?
        .ok_or(PostReportError::PostNotFound)?;
    if post.hidden_at.is_some() {
        return Err(PostReportError::PostNotFound);
    }
    if post.created_by == reporter_id {
        return Err(PostReportError::CannotReportOwnPost);
    }
    // An empty string carries no information; store it as NULL.
    let details = details.filter(|d| !d.is_empty());
    // echobackend `Create`: a repeat pending report by the same user is
    // silently ignored. The conflict target repeats the partial unique index
    // predicate (`idx_post_reports_pending_post_reporter`).
    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO post_reports (id, post_id, reporter_id, reason, details, status, created_at) \
         VALUES ($1, $2, $3, $4, $5, 'pending', $6) \
         ON CONFLICT (post_id, reporter_id) WHERE status = 'pending' DO NOTHING",
        vec![
            Uuid::now_v7().into(),
            post_id.into(),
            reporter_id.into(),
            reason.into(),
            details.into(),
            chrono::Utc::now().into(),
        ],
    );
    db.execute_raw(stmt).await?;
    Ok(())
}

#[derive(FromQueryResult)]
struct GroupRow {
    post_id: Uuid,
    post_title: Option<String>,
    post_slug: Option<String>,
    author_username: Option<String>,
    hidden_at: Option<String>,
    report_count: i64,
    top_reason: String,
    last_reported_at: String,
}

pub async fn list_reported_posts(
    db: &DatabaseConnection,
    status: &str,
    limit: u64,
    offset: u64,
) -> Result<(Vec<PostReportGroup>, u64), PostReportError> {
    if !STATUS_CHOICES.contains(&status) {
        return Err(PostReportError::InvalidReportStatus);
    }
    let total = post_reports::Entity::find()
        .filter(post_reports::Column::Status.eq(status))
        .count(db)
        .await? as u64;
    // Count distinct posts for pagination meta (mirrors echobackend's Distinct count).
    let distinct: Vec<GroupRow> = GroupRow::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT posts.id AS post_id, posts.title AS post_title, posts.slug AS post_slug, users.username AS author_username, to_char(posts.hidden_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS hidden_at, COUNT(*)::bigint AS report_count, MODE() WITHIN GROUP (ORDER BY post_reports.reason) AS top_reason, to_char(MAX(post_reports.created_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_reported_at FROM post_reports JOIN posts ON posts.id = post_reports.post_id LEFT JOIN users ON users.id = posts.created_by AND users.deleted_at IS NULL WHERE post_reports.status = '{}' GROUP BY posts.id, posts.title, posts.slug, users.username, posts.hidden_at ORDER BY COUNT(*) DESC, MAX(post_reports.created_at) DESC LIMIT {} OFFSET {}",
            status.replace('\'', "''"),
            limit,
            offset
        ),
    ))
    .all(db)
    .await?;
    let groups = distinct
        .into_iter()
        .map(|r| PostReportGroup {
            post_id: r.post_id,
            post_title: r.post_title,
            post_slug: r.post_slug,
            author_username: r.author_username,
            hidden_at: r.hidden_at,
            report_count: r.report_count,
            top_reason: r.top_reason,
            last_reported_at: r.last_reported_at,
        })
        .collect();
    // Distinct-post total for meta.
    let distinct_total: i64 = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT COUNT(DISTINCT post_id)::bigint AS value FROM post_reports WHERE status = '{}'",
                status.replace('\'', "''")
            ),
        ))
        .await?
        .and_then(|row| row.try_get("", "value").ok())
        .unwrap_or(total as i64);
    Ok((groups, distinct_total as u64))
}

pub async fn list_post_reports(
    db: &DatabaseConnection,
    post_id: Uuid,
    status: Option<&str>,
    limit: u64,
    offset: u64,
) -> Result<(Vec<SinglePostReportResponse>, u64), PostReportError> {
    if let Some(s) = status
        && !s.is_empty()
        && !STATUS_CHOICES.contains(&s)
    {
        return Err(PostReportError::InvalidReportStatus);
    }
    let mut q = post_reports::Entity::find().filter(post_reports::Column::PostId.eq(post_id));
    if let Some(s) = status
        && !s.is_empty()
    {
        q = q.filter(post_reports::Column::Status.eq(s));
    }
    // Raw LIMIT/OFFSET like echobackend (not page-aligned).
    let (reports, total) = paginate(
        db,
        q.order_by_desc(post_reports::Column::CreatedAt),
        offset,
        limit,
    )
    .await?;
    let reporters = active_users_by_id(db, reports.iter().map(|r| r.reporter_id)).await?;
    let action_ids: HashSet<Uuid> = reports.iter().filter_map(|r| r.action_id).collect();
    let actions: HashMap<Uuid, post_moderation_actions::Model> = if action_ids.is_empty() {
        HashMap::new()
    } else {
        post_moderation_actions::Entity::find()
            .filter(post_moderation_actions::Column::Id.is_in(action_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    };
    let out = reports
        .into_iter()
        .map(|r| {
            // Several reports can share a reporter/action, so clone rather
            // than take from the maps.
            let reporter = reporters.get(&r.reporter_id).cloned();
            // echobackend preloads only "Action" (not "Action.Admin"), so the
            // resolution never carries an admin brief here.
            let resolution = r
                .action_id
                .and_then(|id| actions.get(&id).cloned())
                .map(|a| action_to_response(a, None));
            SinglePostReportResponse {
                id: r.id,
                post_id: r.post_id,
                reason: r.reason,
                details: r.details,
                status: r.status,
                created_at: r.created_at.to_rfc3339(),
                reporter: brief_from_user(reporter),
                resolution,
            }
        })
        .collect();
    Ok((out, total))
}

pub async fn moderate_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    admin_id: Uuid,
    action: &str,
    note: Option<String>,
) -> Result<(), PostReportError> {
    if !ACTION_CHOICES.contains(&action) {
        return Err(PostReportError::InvalidReportStatus);
    }
    let txn = db.begin().await?;
    // SELECT ... FOR UPDATE: two admins acting at once serialize instead of
    // both passing the state check below.
    let post = posts::Entity::find_by_id(post_id)
        .lock_exclusive()
        .one(&txn)
        .await?
        .ok_or(PostReportError::PostNotFound)?;
    match action {
        "hide" => {
            if post.hidden_at.is_some() {
                return Err(PostReportError::PostAlreadyHidden);
            }
        }
        "unhide" => {
            if post.hidden_at.is_none() {
                return Err(PostReportError::PostNotHidden);
            }
        }
        "dismiss" => {
            let pending = post_reports::Entity::find()
                .filter(post_reports::Column::PostId.eq(post_id))
                .filter(post_reports::Column::Status.eq("pending"))
                .count(&txn)
                .await?;
            if pending == 0 {
                return Err(PostReportError::NoPendingReports);
            }
        }
        _ => return Err(PostReportError::InvalidReportStatus),
    }
    let action_id = Uuid::now_v7();
    post_moderation_actions::ActiveModel {
        id: Set(action_id),
        post_id: Set(Some(post_id)),
        post_author_id: Set(Some(post.created_by)),
        admin_id: Set(Some(admin_id)),
        action: Set(action.to_string()),
        // Stored as sent (echobackend keeps "" as "").
        note: Set(note),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&txn)
    .await?;
    match action {
        "hide" => {
            let mut active: posts::ActiveModel = post.into();
            active.hidden_at = Set(Some(chrono::Utc::now().into()));
            active.update(&txn).await?;
            close_pending(&txn, post_id, "resolved", action_id).await?;
        }
        "unhide" => {
            let mut active: posts::ActiveModel = post.into();
            active.hidden_at = Set(None);
            active.update(&txn).await?;
        }
        _ => {
            close_pending(&txn, post_id, "dismissed", action_id).await?;
        }
    }
    txn.commit().await?;
    Ok(())
}

async fn close_pending(
    txn: &sea_orm::DatabaseTransaction,
    post_id: Uuid,
    status: &str,
    action_id: Uuid,
) -> Result<(), PostReportError> {
    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE post_reports SET status = $1, action_id = $2 WHERE post_id = $3 AND status = 'pending'",
        vec![status.into(), action_id.into(), post_id.into()],
    );
    txn.execute_raw(stmt).await?;
    Ok(())
}

pub async fn list_moderation_actions(
    db: &DatabaseConnection,
    post_id: Uuid,
    limit: u64,
    offset: u64,
) -> Result<(Vec<ModerationActionResponse>, u64), PostReportError> {
    let (actions, total) = paginate(
        db,
        post_moderation_actions::Entity::find()
            .filter(post_moderation_actions::Column::PostId.eq(post_id))
            .order_by_desc(post_moderation_actions::Column::CreatedAt),
        offset,
        limit,
    )
    .await?;
    let admins = active_users_by_id(db, actions.iter().filter_map(|a| a.admin_id)).await?;
    let out = actions
        .into_iter()
        .map(|a| {
            let admin = a.admin_id.and_then(|id| admins.get(&id).cloned());
            action_to_response(a, admin)
        })
        .collect();
    Ok((out, total))
}
