//! AI article generation for the create/edit-post page
//! (`POST /api/ai/generate-article`): pure prompt building on top of the
//! OpenRouter client. No database access — the generated draft is streamed
//! back and only saved when the author submits the post.

use crate::clients::openrouter::{self, OpenRouterMessage};
use crate::dto::ai::GenerateArticleRequest;

/// Generation always uses the configured default model at this temperature.
pub const ARTICLE_TEMPERATURE: f64 = 0.7;

pub const TONE_CHOICES: [&str; 4] = ["informative", "professional", "casual", "storytelling"];
pub const LENGTH_CHOICES: [&str; 3] = ["short", "medium", "long"];

pub const SYSTEM_PROMPT: &str = "You are an expert blog writer for pilput, a publishing platform for writers. Write a complete, original, well-structured blog article based on the user's request.

Output rules:
- Respond with HTML only. Do not use Markdown and do not wrap the answer in code fences.
- Do not include <html>, <head>, <body>, <style> or <script> tags, images, or inline styles.
- Start with the article title in a single <h1> element.
- Write the body using only these tags: <h2>, <h3>, <p>, <ul>, <ol>, <li>, <strong>, <em>, <blockquote>, <code>, <pre>, <a href=\"...\">.
- Write in the same language as the user's request.
- Do not add any commentary before or after the article.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArticleTone {
    #[default]
    Informative,
    Professional,
    Casual,
    Storytelling,
}

impl ArticleTone {
    /// Parses a tone choice; `None` for an unknown value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "informative" => Some(Self::Informative),
            "professional" => Some(Self::Professional),
            "casual" => Some(Self::Casual),
            "storytelling" => Some(Self::Storytelling),
            _ => None,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Informative => "informative and clear",
            Self::Professional => "professional and authoritative",
            Self::Casual => "casual and friendly",
            Self::Storytelling => "narrative, storytelling style",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArticleLength {
    Short,
    #[default]
    Medium,
    Long,
}

impl ArticleLength {
    /// Parses a length choice; `None` for an unknown value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "short" => Some(Self::Short),
            "medium" => Some(Self::Medium),
            "long" => Some(Self::Long),
            _ => None,
        }
    }

    pub fn target_words(self) -> u32 {
        match self {
            Self::Short => 400,
            Self::Medium => 800,
            Self::Long => 1500,
        }
    }
}

/// Whether article generation can run (API key + default model configured).
pub fn is_available() -> bool {
    openrouter::is_available(None)
}

/// The model reported in `ai_complete` (the configured default).
pub fn model_name() -> String {
    openrouter::effective_model(None).unwrap_or_default()
}

/// The user message: prompt, tone, length and (when non-blank) the working
/// title, one per line.
pub fn build_user_prompt(
    prompt: &str,
    tone: ArticleTone,
    length: ArticleLength,
    title: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("Write a blog article about: {}", prompt.trim()),
        format!("Tone: {}", tone.description()),
        format!("Length: about {} words", length.target_words()),
    ];
    if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
        lines.push(format!("Working title: {title}"));
    }
    lines.join("\n")
}

/// System prompt followed by the user request, ready for OpenRouter.
pub fn build_messages(req: &GenerateArticleRequest) -> Vec<OpenRouterMessage> {
    vec![
        OpenRouterMessage {
            role: "system".to_string(),
            content: SYSTEM_PROMPT.to_string(),
        },
        OpenRouterMessage {
            role: "user".to_string(),
            content: build_user_prompt(&req.prompt, req.tone(), req.length(), req.title.as_deref()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(json: &str) -> GenerateArticleRequest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn system_prompt_comes_first() {
        let messages = build_messages(&request(r#"{"prompt":"Rust async"}"#));
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, SYSTEM_PROMPT);
        assert!(SYSTEM_PROMPT.starts_with("You are an expert blog writer for pilput"));
        assert!(SYSTEM_PROMPT.contains("\n\nOutput rules:\n- Respond with HTML only."));
        assert!(
            SYSTEM_PROMPT.ends_with("- Do not add any commentary before or after the article.")
        );
        assert_eq!(messages[1].role, "user");
    }

    #[test]
    fn defaults_are_informative_and_medium() {
        let messages = build_messages(&request(r#"{"prompt":"  Rust async  "}"#));
        assert_eq!(
            messages[1].content,
            "Write a blog article about: Rust async\nTone: informative and clear\nLength: about 800 words"
        );
        // Empty strings count as absent.
        let blank = request(r#"{"prompt":"Rust","tone":"","length":"","title":"   "}"#);
        assert_eq!(blank.tone(), ArticleTone::Informative);
        assert_eq!(blank.length(), ArticleLength::Medium);
        assert!(!build_messages(&blank)[1].content.contains("Working title"));
    }

    #[test]
    fn user_prompt_includes_options_and_title() {
        let messages = build_messages(&request(
            r#"{"prompt":"Tea","tone":"storytelling","length":"long","title":"  A cup  "}"#,
        ));
        assert_eq!(
            messages[1].content,
            "Write a blog article about: Tea\nTone: narrative, storytelling style\nLength: about 1500 words\nWorking title: A cup"
        );
    }

    #[test]
    fn tone_and_length_mappings() {
        let tones = [
            ("informative", "informative and clear"),
            ("professional", "professional and authoritative"),
            ("casual", "casual and friendly"),
            ("storytelling", "narrative, storytelling style"),
        ];
        for (raw, description) in tones {
            assert_eq!(ArticleTone::parse(raw).unwrap().description(), description);
        }
        assert_eq!(ArticleTone::parse("angry"), None);
        assert_eq!(ArticleLength::parse("short").unwrap().target_words(), 400);
        assert_eq!(ArticleLength::parse("medium").unwrap().target_words(), 800);
        assert_eq!(ArticleLength::parse("long").unwrap().target_words(), 1500);
        assert_eq!(ArticleLength::parse("huge"), None);
    }
}
