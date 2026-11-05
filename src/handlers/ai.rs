//! `POST /api/ai/generate-article` — streams an AI-written draft article (HTML)
//! for the create/edit-post page. Nothing is stored: the author reviews the
//! draft in the editor and saves it as a normal post.

use super::chat::{sse_data_event, sse_error_event};
use crate::clients::openrouter;
use crate::dto::ai::GenerateArticleRequest;
use crate::http::auth::{AdminUser, require_auth};
use crate::http::error::AppError;
use crate::http::extract::VBind;
use crate::http::rate_limit::{self, RateLimiter};
use crate::models::ai::GeneratedArticle;
use crate::services::article_generation;
use crate::state::AppState;
use axum::{
    Router,
    http::StatusCode,
    middleware,
    response::sse::{Event, Sse},
    routing::post,
};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::StreamExt;

const UNAVAILABLE_MESSAGE: &str = "AI article generation is not available";

/// Streams `ai_chunk` events, then `ai_complete` (`{content, model}`) or
/// `error`, then `data: [DONE]` — the chat frame format without an initial
/// event. `503` when OpenRouter is not configured.
pub async fn generate_article(
    // Super admins only for now (rollout): 403 for everyone else, checked
    // against the database.
    _admin_user: AdminUser,
    VBind(req): VBind<GenerateArticleRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, AppError> {
    if !article_generation::is_available() {
        return Err(AppError::Detailed {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: UNAVAILABLE_MESSAGE.to_string(),
            // Same body as the Go twins' `response.ServiceUnavailable`.
            error: Some("Service Unavailable".to_string()),
        });
    }

    let messages = article_generation::build_messages(&req);
    let model = article_generation::model_name();

    let stream = async_stream::stream! {
        match openrouter::generate_stream(&messages, None, article_generation::ARTICLE_TEMPERATURE)
            .await
        {
            Err(err) => {
                yield sse_error_event(&openrouter::stream_error_message(&err));
            }
            Ok(events) => {
                tokio::pin!(events);
                let mut content = String::new();
                let mut failed: Option<String> = None;

                while let Some(event) = events.next().await {
                    match event {
                        openrouter::StreamEvent::Chunk(chunk) => {
                            content.push_str(&chunk);
                            yield sse_data_event("ai_chunk", &chunk);
                        }
                        openrouter::StreamEvent::Usage(_) => {}
                        openrouter::StreamEvent::Error(err) => {
                            failed = Some(err);
                            break;
                        }
                    }
                }

                if let Some(err) = failed {
                    yield sse_error_event(&openrouter::stream_error_message(&err));
                } else if content.is_empty() {
                    yield sse_error_event("OpenRouter returned an empty response");
                } else {
                    yield sse_data_event("ai_complete", GeneratedArticle { content, model });
                }
            }
        }

        yield Ok(Event::default().data("[DONE]"));
    };

    Ok(Sse::new(stream))
}

/// Merged outside the request timeout (SSE). 20 generations per user/IP per
/// hour; `require_auth` is the outermost layer so a 401 never consumes the
/// budget.
pub fn routes() -> Router<AppState> {
    let trust_proxy =
        std::panic::catch_unwind(|| crate::config::HttpConfig::get().trust_proxy).unwrap_or(false);
    let limiter =
        RateLimiter::new(20, Duration::from_secs(3600), trust_proxy).shared("ai:generate-article");
    Router::new().route(
        "/api/ai/generate-article",
        post(generate_article)
            .route_layer(middleware::from_fn_with_state(
                limiter,
                rate_limit::rate_limit,
            ))
            .route_layer(middleware::from_fn(require_auth)),
    )
}
