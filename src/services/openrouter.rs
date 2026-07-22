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

#[derive(Debug, Clone, Copy, Default, Deserialize)]
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
pub enum StreamEvent {
    Chunk(String),
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
    let client = reqwest::Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|err| format!("failed to build OpenRouter client: {err}"))?;

    let mut request = client
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

    let response = request
        .send()
        .await
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
/// `usage` wins, empty delta contents skipped).
fn decode_stream(response: reqwest::Response) -> impl tokio_stream::Stream<Item = StreamEvent> {
    async_stream::stream! {
        #[derive(Deserialize)]
        struct ChunkPayload {
            #[serde(default)]
            choices: Vec<ChunkChoice>,
            usage: Option<OpenRouterUsage>,
        }
        #[derive(Deserialize)]
        struct ChunkChoice {
            delta: ChunkDelta,
        }
        #[derive(Deserialize)]
        struct ChunkDelta {
            #[serde(default)]
            content: String,
        }

        let mut buffer = String::new();
        let mut byte_stream = response.bytes_stream();
        'read: loop {
            let Some(chunk_result) = tokio_stream::StreamExt::next(&mut byte_stream).await else {
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

            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer.drain(..=pos);
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    break 'read;
                }
                let Ok(payload) = serde_json::from_str::<ChunkPayload>(data) else {
                    continue;
                };
                if let Some(usage) = payload.usage {
                    yield StreamEvent::Usage(usage);
                }
                let content = payload
                    .choices
                    .first()
                    .map(|choice| choice.delta.content.clone())
                    .unwrap_or_default();
                if !content.is_empty() {
                    yield StreamEvent::Chunk(content);
                }
            }
        }
    }
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
