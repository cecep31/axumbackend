//! OpenRouter chat-completions client.
//!
//! Ports echobackend's `openrouter_service.go`: non-streaming generation plus
//! SSE streaming, with the same request/response shapes and lenient defaults
//! (model resolution, temperature fallback, upstream error propagation).

use crate::config::OpenRouterConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct OpenRouterUsage {
    #[serde(default)]
    pub prompt_tokens: i32,
    #[serde(default)]
    pub completion_tokens: i32,
    #[serde(default)]
    pub total_tokens: i32,
}

#[derive(Debug, Deserialize)]
pub struct OpenRouterChoice {
    pub message: OpenRouterMessage,
}

#[derive(Debug, Deserialize)]
pub struct OpenRouterResponse {
    #[serde(default)]
    pub choices: Vec<OpenRouterChoice>,
    #[serde(default)]
    pub usage: OpenRouterUsage,
}

/// Events decoded from an OpenRouter SSE stream.
#[derive(Debug, PartialEq)]
pub enum StreamEvent {
    Chunk(String),
    /// Reasoning ("thinking") text from reasoning models. Not persisted.
    Reasoning(String),
    Usage(OpenRouterUsage),
    Error(String),
}

/// Resolves the model to use: the request model when non-empty, otherwise the
/// configured default. `None` when neither is set.
pub fn effective_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let default = OpenRouterConfig::get().default_model.trim().to_string();
            if default.is_empty() {
                None
            } else {
                Some(default)
            }
        })
}

/// Whether AI generation can be attempted (API key configured + a model
/// resolvable). When `false`, chat endpoints fall back to storing only the
/// user message, exactly like echobackend.
pub fn is_available(model: Option<&str>) -> bool {
    !OpenRouterConfig::get().api_key.trim().is_empty() && effective_model(model).is_some()
}

/// echobackend: temperatures outside `[0, 2]` fall back to `0.7`.
pub fn normalized_temperature(temperature: Option<f64>) -> f64 {
    match temperature {
        Some(value) if (0.0..=2.0).contains(&value) => value,
        _ => 0.7,
    }
}

/// One client for the whole process so connections (and TLS sessions) to
/// OpenRouter are reused. The timeout is set per request from config.
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::LazyLock<reqwest::Client> =
        std::sync::LazyLock::new(reqwest::Client::new);
    &CLIENT
}

async fn call_api(
    messages: &[OpenRouterMessage],
    model: Option<&str>,
    stream: bool,
    temperature: f64,
) -> Result<reqwest::Response, String> {
    let config = OpenRouterConfig::get();
    if config.api_key.trim().is_empty() {
        return Err("OPENROUTER_API_KEY is not configured".to_string());
    }
    let final_model = effective_model(model)
        .ok_or_else(|| "OPENROUTER_DEFAULT_MODEL is not configured".to_string())?;
    let temperature = if (0.0..=2.0).contains(&temperature) {
        temperature
    } else {
        0.7
    };

    let body = serde_json::json!({
        "model": final_model,
        "messages": messages,
        "stream": stream,
        "temperature": temperature,
    });

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let mut request = shared_client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&body);
    if !config.http_referer.is_empty() {
        request = request.header("HTTP-Referer", &config.http_referer);
    }
    if !config.title.is_empty() {
        request = request.header("X-Title", &config.title);
    }

    // A streamed answer (especially with reasoning) can legitimately run past
    // `config.timeout`, so streams only bound the wait for the response head
    // here and the gap between chunks in `decode_stream`.
    let response = if stream {
        tokio::time::timeout(config.timeout, request.send())
            .await
            .map_err(|_| "OpenRouter request timed out".to_string())?
    } else {
        request.timeout(config.timeout).send().await
    }
    .map_err(|err| format!("OpenRouter request failed: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let truncated: String = body.chars().take(4096).collect();
        return Err(format!(
            "OpenRouter API error: {status} {}",
            truncated.trim()
        ));
    }
    Ok(response)
}

/// Non-streaming chat completion (`POST /chat/completions` with
/// `stream: false`).
pub async fn generate_response(
    messages: &[OpenRouterMessage],
    model: Option<&str>,
    temperature: f64,
) -> Result<OpenRouterResponse, String> {
    call_api(messages, model, false, temperature)
        .await?
        .json::<OpenRouterResponse>()
        .await
        .map_err(|err| format!("failed to decode OpenRouter response: {err}"))
}

/// Streaming chat completion. The returned stream yields content chunks and
/// the final token usage; upstream/read failures surface as
/// [`StreamEvent::Error`].
pub async fn generate_stream(
    messages: &[OpenRouterMessage],
    model: Option<&str>,
    temperature: f64,
) -> Result<impl tokio_stream::Stream<Item = StreamEvent>, String> {
    let response = call_api(messages, model, true, temperature).await?;
    Ok(decode_stream(response))
}

/// Decodes an OpenRouter SSE response body into [`StreamEvent`]s, mirroring
/// echobackend's scanner loop (`data: ` lines, `[DONE]` terminator, latest
/// `usage` wins, empty delta contents skipped). SSE comment lines such as
/// `: OPENROUTER PROCESSING` are ignored.
fn decode_stream(response: reqwest::Response) -> impl tokio_stream::Stream<Item = StreamEvent> {
    async_stream::stream! {
        let idle_timeout = OpenRouterConfig::get().timeout;
        let mut buffer = Vec::new();
        let mut byte_stream = response.bytes_stream();
        'read: loop {
            let next = tokio::time::timeout(
                idle_timeout,
                tokio_stream::StreamExt::next(&mut byte_stream),
            )
            .await;
            let Ok(next) = next else {
                yield StreamEvent::Error("OpenRouter stream timed out".to_string());
                break;
            };
            let Some(chunk_result) = next else {
                break;
            };
            let bytes = match chunk_result {
                Ok(bytes) => bytes,
                Err(err) => {
                    yield StreamEvent::Error(format!(
                        "failed to read OpenRouter stream: {err}"
                    ));
                    break;
                }
            };

            buffer.extend_from_slice(&bytes);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                let line_str = String::from_utf8_lossy(&line_bytes);
                let line = line_str.trim();
                let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
                    continue;
                };
                if data == "[DONE]" {
                    break 'read;
                }
                let mut stop = false;
                for event in parse_stream_data(data) {
                    stop |= matches!(event, StreamEvent::Error(_));
                    yield event;
                }
                if stop {
                    break 'read;
                }
            }
        }
    }
}

/// Parses one SSE `data:` payload of an OpenRouter chat-completions stream.
///
/// Decoded leniently via `serde_json::Value` so an unexpected field shape
/// never drops the whole chunk. Per the OpenRouter docs:
/// - reasoning streams in `choices[0].delta.reasoning_details` (items of type
///   `reasoning.text` / `reasoning.summary`; `reasoning.encrypted` carries no
///   readable text). Some providers also send a plain `delta.reasoning`
///   string; it is used only when `reasoning_details` has no readable text,
///   so the same reasoning is never emitted twice.
/// - mid-stream failures arrive as a top-level `error` object (HTTP stays 200).
fn parse_stream_data(data: &str) -> Vec<StreamEvent> {
    use serde_json::Value;

    let Ok(payload) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let mut events = Vec::new();

    if let Some(error) = payload.get("error").filter(|e| !e.is_null()) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .unwrap_or("OpenRouter stream failed");
        events.push(StreamEvent::Error(message.to_string()));
        return events;
    }

    if let Some(usage) = payload
        .get("usage")
        .filter(|u| !u.is_null())
        .and_then(|u| serde_json::from_value::<OpenRouterUsage>(u.clone()).ok())
    {
        events.push(StreamEvent::Usage(usage));
    }

    let Some(delta) = payload.pointer("/choices/0/delta") else {
        return events;
    };

    let from_details: String = delta
        .get("reasoning_details")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                    Some("reasoning.text") => item.get("text").and_then(Value::as_str),
                    Some("reasoning.summary") => item.get("summary").and_then(Value::as_str),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let reasoning = if from_details.is_empty() {
        delta
            .get("reasoning")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        from_details
    };
    if !reasoning.is_empty() {
        events.push(StreamEvent::Reasoning(reasoning));
    }

    if let Some(content) = delta
        .get("content")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
    {
        events.push(StreamEvent::Chunk(content.to_string()));
    }

    events
}

/// Extracts a client-safe message from an upstream error body, mirroring
/// echobackend's `streamErrorMessage` (`{"error":{"message": ...}}`).
pub fn stream_error_message(err: &str) -> String {
    if let Some(idx) = err.find("{\"error\"") {
        #[derive(Deserialize)]
        struct ErrorPayload {
            error: ErrorBody,
        }
        #[derive(Deserialize)]
        struct ErrorBody {
            message: String,
        }

        if let Ok(payload) = serde_json::from_str::<ErrorPayload>(&err[idx..])
            && !payload.error.message.is_empty()
        {
            return payload.error.message;
        }
    }
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::{OpenRouterUsage, StreamEvent, parse_stream_data};

    #[test]
    fn content_chunk() {
        let data = r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hi"}}]}"#;
        assert_eq!(
            parse_stream_data(data),
            vec![StreamEvent::Chunk("Hi".into())]
        );
    }

    #[test]
    fn null_content_and_reasoning_are_skipped() {
        let data = r#"{"choices":[{"delta":{"content":null,"reasoning":null}}]}"#;
        assert!(parse_stream_data(data).is_empty());
    }

    #[test]
    fn reasoning_details_text_and_summary() {
        let data = r#"{"choices":[{"delta":{"content":"","reasoning":"think","reasoning_details":[
            {"type":"reasoning.text","text":"think","format":"anthropic-claude-v1","index":0},
            {"type":"reasoning.encrypted","data":"xyz"}
        ]}}]}"#;
        // Same text in both fields is emitted once.
        assert_eq!(
            parse_stream_data(data),
            vec![StreamEvent::Reasoning("think".into())]
        );

        let data = r#"{"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.summary","summary":"sum"}]}}]}"#;
        assert_eq!(
            parse_stream_data(data),
            vec![StreamEvent::Reasoning("sum".into())]
        );
    }

    #[test]
    fn plain_reasoning_string_fallback() {
        let data = r#"{"choices":[{"delta":{"content":"","reasoning":"step 1"}}]}"#;
        assert_eq!(
            parse_stream_data(data),
            vec![StreamEvent::Reasoning("step 1".into())]
        );
    }

    #[test]
    fn encrypted_only_reasoning_emits_nothing() {
        let data = r#"{"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","data":"x"}]}}]}"#;
        assert!(parse_stream_data(data).is_empty());
    }

    #[test]
    fn usage_chunk() {
        let data = r#"{"choices":[{"index":0,"delta":{"content":"","role":"assistant"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8,"completion_tokens_details":{"reasoning_tokens":2}}}"#;
        assert_eq!(
            parse_stream_data(data),
            vec![StreamEvent::Usage(OpenRouterUsage {
                prompt_tokens: 3,
                completion_tokens: 5,
                total_tokens: 8
            })]
        );
    }

    #[test]
    fn mid_stream_error() {
        let data = r#"{"error":{"code":502,"message":"Provider disconnected unexpectedly"},
            "choices":[{"index":0,"delta":{"content":""},"finish_reason":"error"}]}"#;
        assert_eq!(
            parse_stream_data(data),
            vec![StreamEvent::Error(
                "Provider disconnected unexpectedly".into()
            )]
        );
    }

    #[test]
    fn invalid_json_is_ignored() {
        assert!(parse_stream_data("not json").is_empty());
    }
}
