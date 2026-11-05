use serde::Serialize;

/// `data` of the `ai_complete` event of `POST /api/ai/generate-article`.
#[derive(Debug, Serialize)]
pub struct GeneratedArticle {
    pub content: String,
    pub model: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chat::ChatStreamEvent;
    use serde_json::json;

    #[test]
    fn article_stream_event_shapes() {
        let chunk = ChatStreamEvent {
            kind: "ai_chunk",
            data: "<h1>Hi</h1>",
        };
        assert_eq!(
            serde_json::to_value(&chunk).unwrap(),
            json!({ "type": "ai_chunk", "data": "<h1>Hi</h1>" })
        );

        let complete = ChatStreamEvent {
            kind: "ai_complete",
            data: GeneratedArticle {
                content: "<h1>Hi</h1><p>Body</p>".into(),
                model: "openrouter/free".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&complete).unwrap(),
            json!({
                "type": "ai_complete",
                "data": { "content": "<h1>Hi</h1><p>Body</p>", "model": "openrouter/free" },
            })
        );

        let error = ChatStreamEvent {
            kind: "error",
            data: "OpenRouter returned an empty response",
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "type": "error", "data": "OpenRouter returned an empty response" })
        );
    }
}
