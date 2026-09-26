//! Guards the posts <-> tags many-to-many relation: a wrong `via()` joins
//! `posts_to_tags` twice, which PostgreSQL rejects with 42712.

use axumbackend::entities::{posts, tags};
use sea_orm::{DbBackend, ModelTrait, QueryTrait, prelude::Uuid};

fn post() -> posts::Model {
    posts::Model {
        id: Uuid::nil(),
        created_at: None,
        updated_at: None,
        title: String::new(),
        created_by: Uuid::nil(),
        body: None,
        slug: String::new(),
        photo_url: None,
        published: None,
        published_at: None,
        view_count: None,
        like_count: None,
        bookmark_count: None,
    }
}

#[test]
fn post_tags_joins_junction_once() {
    let sql = post()
        .find_related(tags::Entity)
        .build(DbBackend::Postgres)
        .to_string();
    assert_eq!(sql.matches(r#"JOIN "posts_to_tags""#).count(), 1, "{sql}");
    assert!(sql.contains(r#"INNER JOIN "posts""#), "{sql}");
}

#[test]
fn tag_posts_joins_junction_once() {
    let tag = tags::Model {
        id: 1,
        name: String::new(),
        created_at: None,
    };
    let sql = tag
        .find_related(posts::Entity)
        .build(DbBackend::Postgres)
        .to_string();
    assert_eq!(sql.matches(r#"JOIN "posts_to_tags""#).count(), 1, "{sql}");
    assert!(sql.contains(r#"INNER JOIN "tags""#), "{sql}");
}
