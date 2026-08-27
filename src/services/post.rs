use crate::entities::{posts, posts_to_tags, tags, users};
use crate::models::post::{Post, SitemapPost};
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, ExprTrait,
    FromQueryResult, IntoActiveModel, JoinType, ModelTrait, Order, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, RelationTrait, Set,
};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub enum SortDirection {
    Asc,
    Desc,
}

fn validate_order_field(order_by: Option<&str>) -> posts::Column {
    match order_by {
        Some("id") => posts::Column::Id,
        Some("title") => posts::Column::Title,
        Some("updated_at") => posts::Column::UpdatedAt,
        Some("view_count") => posts::Column::ViewCount,
        Some("like_count") => posts::Column::LikeCount,
        Some("bookmark_count") => posts::Column::BookmarkCount,
        _ => posts::Column::CreatedAt,
    }
}

fn get_order_dir(dir: Option<SortDirection>) -> Order {
    match dir {
        Some(SortDirection::Asc) => Order::Asc,
        _ => Order::Desc,
    }
}

async fn hydrate_post(
    post: &posts::Model,
    user: Option<users::Model>,
    tags: Vec<tags::Model>,
    truncate_body: bool,
) -> Result<Post, DbErr> {
    Ok(Post::from_entity(post.clone(), user, tags, truncate_body))
}

async fn hydrate_posts(
    db: &DatabaseConnection,
    posts: Vec<posts::Model>,
    truncate_body: bool,
) -> Result<Vec<Post>, DbErr> {
    if posts.is_empty() {
        return Ok(Vec::new());
    }

    let post_ids: Vec<uuid::Uuid> = posts.iter().map(|p| p.id).collect();

    let users_map: std::collections::HashMap<uuid::Uuid, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(posts.iter().map(|p| p.created_by)))
        .all(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();

    let tags_map: std::collections::HashMap<uuid::Uuid, Vec<tags::Model>> = {
        #[derive(sea_orm::FromQueryResult)]
        struct PostTagRow {
            post_id: uuid::Uuid,
            tag_id: i32,
            tag_name: String,
        }

        let params = sea_orm::sea_query::Value::from(post_ids.clone());
        let rows: Vec<PostTagRow> = PostTagRow::find_by_statement(
            sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Postgres,
                "SELECT ptt.post_id, t.id AS tag_id, t.name AS tag_name FROM posts_to_tags ptt INNER JOIN tags t ON ptt.tag_id = t.id WHERE ptt.post_id = ANY($1)",
                vec![params],
            ),
        )
        .all(db)
        .await?;

        let mut map: std::collections::HashMap<uuid::Uuid, Vec<tags::Model>> =
            std::collections::HashMap::new();
        for row in rows {
            let tag = tags::Model {
                id: row.tag_id,
                name: row.tag_name,
                created_at: None,
            };
            map.entry(row.post_id).or_default().push(tag);
        }
        map
    };

    let mut hydrated = Vec::with_capacity(posts.len());
    for post in &posts {
        let user = users_map.get(&post.created_by).cloned();
        let tags = tags_map.get(&post.id).cloned().unwrap_or_default();
        hydrated.push(hydrate_post(post, user, tags, truncate_body).await?);
    }

    Ok(hydrated)
}

/// Full filter for `GET /api/posts`, mirroring echobackend's `PostQueryFilter`
/// (`internal/dto/post.go` + `repository.GetPostsFiltered`).
pub struct PostsFilter<'a> {
    pub offset: i64,
    pub limit: i64,
    pub search: Option<&'a str>,
    pub sort_by: Option<&'a str>,
    pub sort_order: Option<SortDirection>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub created_by: Option<uuid::Uuid>,
    pub published: Option<bool>,
    pub tags: Vec<String>,
}

pub async fn get_all_posts(
    db: &DatabaseConnection,
    filter: PostsFilter<'_>,
) -> Result<(Vec<Post>, i64), DbErr> {
    // echobackend's `activePostUserJoin`: posts by soft-deleted users are hidden.
    let mut query = posts::Entity::find()
        .join(JoinType::InnerJoin, posts::Relation::Users.def())
        .filter(posts::Column::DeletedAt.is_null())
        .filter(users::Column::DeletedAt.is_null());

    if let Some(search) = filter.search.map(str::trim).filter(|s| !s.is_empty()) {
        // echobackend: `posts.title ILIKE '%search%' AND posts.published = true`.
        query = query
            .filter(Expr::col(posts::Column::Title).ilike(format!("%{search}%")))
            .filter(posts::Column::Published.eq(true));
    } else {
        query = query.filter(posts::Column::Published.eq(true));
        if let Some(published) = filter.published {
            // echobackend applies `published` on top of the default
            // `published = true` filter, so `published=false` yields an empty
            // page; preserved here for parity.
            query = query.filter(posts::Column::Published.eq(published));
        }
    }

    if let Some(start_date) = filter.start_date {
        let start = start_date
            .and_hms_opt(0, 0, 0)
            .expect("valid start of day")
            .and_utc();
        query = query.filter(posts::Column::CreatedAt.gte(start));
    }
    if let Some(end_date) = filter.end_date {
        // echobackend compares `created_at <= 'end_date'` directly, i.e.
        // midnight at the start of `end_date`.
        let end = end_date
            .and_hms_opt(0, 0, 0)
            .expect("valid start of day")
            .and_utc();
        query = query.filter(posts::Column::CreatedAt.lte(end));
    }
    if let Some(created_by) = filter.created_by {
        query = query.filter(posts::Column::CreatedBy.eq(created_by));
    }
    if !filter.tags.is_empty() {
        query = query
            .join(JoinType::InnerJoin, posts::Relation::PostsToTags.def())
            .join(JoinType::InnerJoin, posts_to_tags::Relation::Tags.def())
            .filter(tags::Column::Name.is_in(filter.tags.iter().cloned()));
    }

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by(
            validate_order_field(filter.sort_by),
            get_order_dir(filter.sort_order),
        )
        .limit(Ord::max(filter.limit, 0) as u64)
        .offset(Ord::max(filter.offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

pub async fn get_post_by_id(
    db: &DatabaseConnection,
    id: uuid::Uuid,
) -> Result<Option<Post>, DbErr> {
    let post = posts::Entity::find_by_id(id)
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match post {
        Some(post) => {
            let user = post.find_related(users::Entity).one(db).await?;
            let tags = post.find_related(tags::Entity).all(db).await?;
            Ok(Some(hydrate_post(&post, user, tags, false).await?))
        }
        None => Ok(None),
    }
}

pub async fn get_posts_by_username(
    db: &DatabaseConnection,
    username: &str,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Post>, i64), DbErr> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let Some(user) = user else {
        return Ok((Vec::new(), 0));
    };

    // echobackend's GetPostByUsername has no `published` filter — it also
    // surfaces the user's unpublished/draft posts publicly.
    let query = user
        .clone()
        .find_related(posts::Entity)
        .filter(posts::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

pub async fn get_posts_by_created_by(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Post>, i64), DbErr> {
    let query = posts::Entity::find()
        .filter(posts::Column::CreatedBy.eq(user_id))
        .filter(posts::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

/// Mirrors echobackend's `GetPostsForYou`: posts authored by `user_id` or by
/// anyone `user_id` follows, published, newest first.
pub async fn get_posts_for_you(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Post>, i64), DbErr> {
    let following_ids = crate::entities::user_follows::Entity::find()
        .select_only()
        .column(crate::entities::user_follows::Column::FollowingId)
        .filter(crate::entities::user_follows::Column::FollowerId.eq(user_id))
        .filter(crate::entities::user_follows::Column::DeletedAt.is_null())
        .into_query();

    let query = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::DeletedAt.is_null())
        .filter(
            posts::Column::CreatedBy
                .eq(user_id)
                .or(posts::Column::CreatedBy.in_subquery(following_ids)),
        );

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

pub async fn get_post_by_id_for_author(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<Post>, DbErr> {
    let post = posts::Entity::find_by_id(post_id)
        .filter(posts::Column::CreatedBy.eq(user_id))
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match post {
        Some(post) => {
            let user = post.find_related(users::Entity).one(db).await?;
            let tags = post.find_related(tags::Entity).all(db).await?;
            Ok(Some(hydrate_post(&post, user, tags, false).await?))
        }
        None => Ok(None),
    }
}

pub async fn get_random_posts(db: &DatabaseConnection, limit: i64) -> Result<Vec<Post>, DbErr> {
    let post_models = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::DeletedAt.is_null())
        .order_by(sea_orm::sea_query::Expr::cust("RANDOM()"), Order::Asc)
        .limit(Ord::max(limit, 0) as u64)
        .all(db)
        .await?;

    hydrate_posts(db, post_models, true).await
}

pub async fn get_trending_posts(db: &DatabaseConnection, limit: i64) -> Result<Vec<Post>, DbErr> {
    let post_models = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::DeletedAt.is_null())
        .order_by(
            sea_orm::sea_query::Expr::cust("like_count * 2 + bookmark_count * 2 + view_count"),
            Order::Desc,
        )
        .limit(Ord::max(limit, 0) as u64)
        .all(db)
        .await?;

    hydrate_posts(db, post_models, true).await
}

pub async fn get_posts_for_sitemap(
    db: &DatabaseConnection,
    limit: i64,
) -> Result<Vec<SitemapPost>, DbErr> {
    let post_models = posts::Entity::find()
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::DeletedAt.is_null())
        .order_by_desc(posts::Column::CreatedAt)
        .limit(Ord::max(limit, 0) as u64)
        .all(db)
        .await?;

    let users_by_id: std::collections::HashMap<uuid::Uuid, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(post_models.iter().map(|post| post.created_by)))
        .filter(users::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|user| (user.id, user))
        .collect();

    let sitemap = post_models
        .into_iter()
        .filter_map(|post| {
            users_by_id
                .get(&post.created_by)
                .cloned()
                .map(|user| SitemapPost::from_entities(post, user))
        })
        .collect();

    Ok(sitemap)
}

pub async fn get_post_by_username_and_slug(
    db: &DatabaseConnection,
    username: &str,
    slug: &str,
) -> Result<Option<Post>, DbErr> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let Some(user) = user else {
        return Ok(None);
    };

    let post = user
        .find_related(posts::Entity)
        .filter(posts::Column::Slug.eq(slug))
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match post {
        Some(post) => {
            let user = post.find_related(users::Entity).one(db).await?;
            let tags = post.find_related(tags::Entity).all(db).await?;
            Ok(Some(hydrate_post(&post, user, tags, false).await?))
        }
        None => Ok(None),
    }
}

async fn find_or_create_tag(db: &DatabaseConnection, name: &str) -> Result<tags::Model, DbErr> {
    if let Some(tag) = tags::Entity::find()
        .filter(tags::Column::Name.eq(name))
        .one(db)
        .await?
    {
        return Ok(tag);
    }

    tags::ActiveModel {
        name: Set(name.to_string()),
        created_at: Set(Some(Utc::now().into())),
        ..Default::default()
    }
    .insert(db)
    .await
}

fn normalize_tag_names(tag_names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for name in tag_names
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        if seen.insert(name.to_string()) {
            normalized.push(name.to_string());
        }
    }

    normalized
}

async fn replace_post_tags(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    tag_names: &[String],
) -> Result<(), DbErr> {
    posts_to_tags::Entity::delete_many()
        .filter(posts_to_tags::Column::PostId.eq(post_id))
        .exec(db)
        .await?;

    for name in normalize_tag_names(tag_names) {
        let tag = find_or_create_tag(db, &name).await?;
        posts_to_tags::ActiveModel {
            post_id: Set(post_id),
            tag_id: Set(tag.id),
        }
        .insert(db)
        .await?;
    }

    Ok(())
}

pub struct CreatePostInput {
    pub title: String,
    pub photo_url: Option<String>,
    pub slug: String,
    pub body: String,
    pub published: bool,
    pub tags: Vec<String>,
}

pub struct UpdatePostInput {
    pub title: Option<String>,
    pub photo_url: Option<String>,
    pub slug: Option<String>,
    pub body: Option<String>,
    pub published: Option<bool>,
    pub tags: Option<Vec<String>>,
}

pub async fn create_post(
    db: &DatabaseConnection,
    input: CreatePostInput,
    creator_id: uuid::Uuid,
) -> Result<Post, DbErr> {
    let now = Utc::now();
    let post = posts::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        title: Set(input.title),
        created_by: Set(creator_id),
        body: Set(Some(input.body)),
        slug: Set(input.slug),
        photo_url: Set(Some(input.photo_url.unwrap_or_default())),
        published: Set(Some(input.published)),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
        view_count: Set(Some(0)),
        like_count: Set(Some(0)),
        bookmark_count: Set(Some(0)),
        ..Default::default()
    }
    .insert(db)
    .await?;

    replace_post_tags(db, post.id, &input.tags).await?;
    let user = post.find_related(users::Entity).one(db).await?;
    let tags = post.find_related(tags::Entity).all(db).await?;
    hydrate_post(&post, user, tags, false).await
}

pub async fn is_author(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<bool>, DbErr> {
    let post = posts::Entity::find_by_id(post_id)
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    Ok(post.map(|post| post.created_by == user_id))
}

pub async fn update_post(
    db: &DatabaseConnection,
    post_id: uuid::Uuid,
    input: UpdatePostInput,
) -> Result<Option<Post>, DbErr> {
    let Some(post) = posts::Entity::find_by_id(post_id)
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let mut active = post.into_active_model();
    if let Some(title) = input.title.filter(|value| !value.trim().is_empty()) {
        active.title = Set(title);
    }
    if let Some(body) = input.body.filter(|value| !value.trim().is_empty()) {
        active.body = Set(Some(body));
    }
    if let Some(slug) = input.slug.filter(|value| !value.trim().is_empty()) {
        active.slug = Set(slug);
    }
    if input.photo_url.is_some() {
        active.photo_url = Set(input.photo_url);
    }
    if let Some(published) = input.published {
        active.published = Set(Some(published));
    }
    active.updated_at = Set(Some(Utc::now().into()));

    // echobackend's UpdatePost repository never touches posts_to_tags — the
    // `tags` field on the update request is intentionally a no-op here too.
    let post = active.update(db).await?;

    let user = post.find_related(users::Entity).one(db).await?;
    let tags = post.find_related(tags::Entity).all(db).await?;
    Ok(Some(hydrate_post(&post, user, tags, false).await?))
}

pub async fn soft_delete_post(db: &DatabaseConnection, post_id: uuid::Uuid) -> Result<bool, DbErr> {
    let Some(post) = posts::Entity::find_by_id(post_id)
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(false);
    };

    let mut active = post.into_active_model();
    active.deleted_at = Set(Some(Utc::now().into()));
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;
    Ok(true)
}

pub async fn get_posts_by_tag(
    db: &DatabaseConnection,
    tag_name: &str,
    offset: i64,
    limit: i64,
    _search: Option<&str>,
    order_by: Option<&str>,
    order_direction: Option<SortDirection>,
) -> Result<(Vec<Post>, i64), DbErr> {
    let tag = tags::Entity::find()
        .filter(tags::Column::Name.eq(tag_name))
        .one(db)
        .await?;

    let Some(tag) = tag else {
        return Ok((Vec::new(), 0));
    };

    let query = tag
        .find_related(posts::Entity)
        .filter(posts::Column::Published.eq(true))
        .filter(posts::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let post_models = query
        .order_by(
            validate_order_field(order_by),
            get_order_dir(order_direction),
        )
        .limit(Ord::max(limit, 0) as u64)
        .offset(Ord::max(offset, 0) as u64)
        .all(db)
        .await?;

    Ok((hydrate_posts(db, post_models, true).await?, total))
}

#[cfg(test)]
mod tests {
    use super::normalize_tag_names;

    #[test]
    fn normalize_tag_names_trims_filters_empty_and_deduplicates() {
        let input = vec![
            " rust ".to_string(),
            "".to_string(),
            "axum".to_string(),
            "rust".to_string(),
            "   ".to_string(),
            "sea-orm".to_string(),
        ];

        let normalized = normalize_tag_names(&input);

        assert_eq!(normalized, vec!["rust", "axum", "sea-orm"]);
    }
}
