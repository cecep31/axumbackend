use crate::entities::tags;
use crate::models::tag::{SitemapTag, Tag, TrendingTag};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, IntoActiveModel,
    QueryOrder, Set,
};

pub async fn get_tag_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<Tag>, DbErr> {
    let tag = tags::Entity::find_by_id(id).one(db).await?;
    Ok(tag.map(Into::into))
}

pub async fn get_tags_for_sitemap(
    db: &DatabaseConnection,
    limit: i64,
) -> Result<Vec<SitemapTag>, DbErr> {
    #[derive(FromQueryResult)]
    struct Row {
        name: String,
        created_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    }

    let rows: Vec<Row> = Row::find_by_statement(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        r#"
        SELECT t.name, t.created_at
        FROM tags t
        INNER JOIN posts_to_tags ptt ON ptt.tag_id = t.id
        INNER JOIN posts p ON p.id = ptt.post_id
        WHERE p.published = true
        GROUP BY t.id, t.name, t.created_at
        ORDER BY t.name ASC
        LIMIT $1
        "#,
        vec![limit.max(0).into()],
    ))
    .all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| SitemapTag {
            name: row.name,
            created_at: row.created_at.map(|dt| dt.with_timezone(&Utc)),
        })
        .collect())
}

pub async fn get_all_tags(db: &DatabaseConnection) -> Result<Vec<Tag>, DbErr> {
    let tag_models = tags::Entity::find()
        .order_by_asc(tags::Column::Name)
        .all(db)
        .await?;

    Ok(tag_models.into_iter().map(Into::into).collect())
}

pub async fn create_tag(db: &DatabaseConnection, name: String) -> Result<Tag, DbErr> {
    let tag = tags::ActiveModel {
        name: Set(name),
        created_at: Set(Some(Utc::now().into())),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(tag.into())
}

pub async fn update_tag(
    db: &DatabaseConnection,
    id: i32,
    name: String,
) -> Result<Option<Tag>, DbErr> {
    let Some(tag) = tags::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };

    let mut active = tag.into_active_model();
    active.name = Set(name);
    let tag = active.update(db).await?;

    Ok(Some(tag.into()))
}

pub async fn delete_tag(db: &DatabaseConnection, id: i32) -> Result<bool, DbErr> {
    let result = tags::Entity::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

pub async fn get_trending_tags(
    db: &DatabaseConnection,
    limit: i64,
) -> Result<Vec<TrendingTag>, DbErr> {
    #[derive(FromQueryResult)]
    struct Row {
        id: i32,
        name: String,
        total_views: i64,
        total_likes: i64,
        trending_score: i64,
    }

    let rows: Vec<Row> = Row::find_by_statement(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        r#"
        SELECT t.id,
               t.name,
               COALESCE(SUM(p.view_count), 0)::bigint AS total_views,
               COALESCE(SUM(p.like_count), 0)::bigint AS total_likes,
               COALESCE(SUM(p.like_count * 2 + p.bookmark_count * 2 + p.view_count), 0)::bigint AS trending_score,
               COUNT(ptt.post_id)::bigint AS post_count
        FROM tags t
        INNER JOIN posts_to_tags ptt ON ptt.tag_id = t.id
        INNER JOIN posts p ON p.id = ptt.post_id
        INNER JOIN users u ON u.id = p.created_by AND u.deleted_at IS NULL
        WHERE p.published = true AND p.deleted_at IS NULL
        GROUP BY t.id, t.name
        ORDER BY trending_score DESC, post_count DESC, t.name ASC
        LIMIT $1
        "#,
        vec![limit.max(0).into()],
    ))
    .all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TrendingTag {
            id: row.id,
            name: row.name,
            total_views: row.total_views,
            total_likes: row.total_likes,
            trending_score: row.trending_score,
        })
        .collect())
}
