use crate::dto::chat::{
    CreateChatConversationRequest, CreateChatConversationStreamRequest, CreateChatMessageRequest,
    UpdateChatConversationRequest,
};
use crate::entities::{chat_conversations, chat_messages};
use crate::models::chat::{
    ChatConversationResponse, ChatMessageResponse, conversation_response, message_response,
};
use crate::services::openrouter::{self, OpenRouterMessage, OpenRouterUsage};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

#[derive(Debug)]
pub enum ChatError {
    Db(DbErr),
    ConversationNotFound,
    ConversationNotOwned,
    MessageNotFound,
}

impl From<DbErr> for ChatError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

/// Outcome of a streaming chat request, mirroring echobackend: either the AI
/// fallback (only the user message was stored) or everything needed to run
/// the SSE stream.
pub enum StreamOutcome {
    /// AI unavailable — respond `201` with an array holding the user message.
    Fallback(ChatMessageResponse),
    /// AI available — stream `ai_chunk` / `ai_complete` events via SSE.
    Stream(StreamPreparation),
}

pub struct StreamPreparation {
    pub user_id: Uuid,
    pub conversation_id: Uuid,
    pub user_message: ChatMessageResponse,
    pub context_messages: Vec<OpenRouterMessage>,
    pub model: Option<String>,
    pub temperature: f64,
}

async fn owned_conversation(
    db: &DatabaseConnection,
    id: Uuid,
    user_id: Uuid,
) -> Result<chat_conversations::Model, ChatError> {
    let conversation = chat_conversations::Entity::find_by_id(id)
        .filter(chat_conversations::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(ChatError::ConversationNotFound)?;
    if conversation.user_id != user_id {
        return Err(ChatError::ConversationNotOwned);
    }
    Ok(conversation)
}

fn normalized_role(role: Option<String>) -> String {
    match role.as_deref().map(str::trim) {
        Some(trimmed) if !trimmed.is_empty() => trimmed.to_string(),
        _ => "user".to_string(),
    }
}

const DEFAULT_CONVERSATION_TITLE: &str = "New conversation";
/// Judul otomatis dari pesan pertama: pendek ala daftar chat.
const MAX_TITLE_WORDS: usize = 8;
const MAX_TITLE_CHARS: usize = 60;
/// Judul eksplisit: ikuti kapasitas kolom DB (varchar 255).
const MAX_EXPLICIT_TITLE_CHARS: usize = 255;

fn build_conversation_title(title: Option<String>, content: &str) -> String {
    if let Some(title) = title {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            // Judul eksplisit dari client: hormati isinya, cukup rapikan
            // whitespace dan batasi ke kapasitas kolom DB.
            return truncate_chars(&normalize_space(trimmed), MAX_EXPLICIT_TITLE_CHARS);
        }
    }
    let normalized = normalize_space(content);
    if normalized.is_empty() {
        return DEFAULT_CONVERSATION_TITLE.to_string();
    }
    // Ambil baris/kalimat pertama agar tidak kepotong tengah kalimat.
    let candidate = first_sentence(&normalized);
    let mut words: Vec<&str> = candidate.split_whitespace().collect();
    words = strip_leading_fillers(words);
    words = strip_trailing_fillers(words);
    if words.is_empty() {
        words = candidate.split_whitespace().collect();
    }
    let title = truncate_words(&words, MAX_TITLE_WORDS, MAX_TITLE_CHARS);
    if title.is_empty() {
        return DEFAULT_CONVERSATION_TITLE.to_string();
    }
    title
}

/// Memadatkan semua whitespace menjadi satu spasi.
fn normalize_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Memotong string per char (aman untuk UTF-8).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

const TITLE_TRIM_CHARS: &[char] = &[
    ' ', '\t', '"', '\'', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '.', ',', ':', ';', '!',
    '?', '\u{2026}', '-', '\u{2013}', '\u{2014}',
];

/// Mengambil baris pertama, lalu potong di akhir kalimat pertama
/// (. ! ? …) bila prefix-nya sudah cukup bermakna (>=3 kata).
/// Juga membersihkan prefix markdown/list seperti "# ", "> ", "- ", "1. ".
fn first_sentence(s: &str) -> String {
    let mut s = s.split('\n').next().unwrap_or("").trim();
    if s.is_empty() {
        return String::new();
    }
    s = s.trim_start_matches([
        '#', '>', '*', '-', '\u{2022}', '\u{2013}', '\u{2014}', ' ', '\t',
    ]);
    if let Some(dot) = s.find(". ") {
        let head = s[..dot].trim();
        if head.split_whitespace().count() >= 3 {
            s = head;
        }
    }
    let mut end: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c == '!' || c == '?' || c == '\u{2026}' || c == '.' {
            end = Some(i + c.len_utf8());
            break;
        }
    }
    if let Some(end) = end {
        let head = s[..end].trim();
        if head.split_whitespace().count() >= 3 {
            s = head;
        }
    }
    // Strip "1. ", "12) " ala numbered list di awal.
    let digits_len = s
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();
    if digits_len > 0 && digits_len < s.len() {
        let rest = &s[digits_len..];
        if let Some(c) = rest.chars().next()
            && (c == '.' || c == ')')
        {
            let after = rest[c.len_utf8()..].trim();
            if !after.is_empty() {
                s = after;
            }
        }
    }
    s.trim_matches(TITLE_TRIM_CHARS).to_string()
}

/// Kata pengisi di awal pesan (ID/EN) yang buruk untuk judul.
fn is_leading_filler(word: &str) -> bool {
    matches!(
        word,
        "tolong"
            | "mohon"
            | "please"
            | "pls"
            | "plis"
            | "coba"
            | "cobalah"
            | "bisakah"
            | "bisa"
            | "bolehkah"
            | "gimana"
            | "bagaimana"
            | "cara"
            | "buatkan"
            | "buatin"
            | "bikinkan"
            | "bikinin"
            | "tuliskan"
            | "tulis"
            | "tuliskanlah"
            | "jelaskan"
            | "jelasin"
            | "jelasken"
            | "kasih"
            | "kasi"
            | "berikan"
            | "beritahu"
            | "beritahukan"
            | "tunjukkan"
            | "tunjukin"
            | "apa"
            | "apakah"
            | "itu"
            | "ini"
            | "halo"
            | "haloo"
            | "hallo"
            | "hai"
            | "hi"
            | "hello"
    )
}

/// Kata pengisi di akhir pesan yang buruk untuk judul.
fn is_trailing_filler(word: &str) -> bool {
    matches!(
        word,
        "ya" | "yah" | "dong" | "donk" | "sih" | "deh" | "loh" | "lho" | "kah" | "tuh"
    )
}

fn strip_leading_fillers(mut words: Vec<&str>) -> Vec<&str> {
    let mut stripped = 0;
    while stripped < 2 {
        let Some(first) = words.first() else {
            break;
        };
        if !is_leading_filler(&first.to_lowercase()) {
            break;
        }
        words.remove(0);
        stripped += 1;
    }
    words
}

fn strip_trailing_fillers(mut words: Vec<&str>) -> Vec<&str> {
    while words.len() > 3 {
        let Some(last) = words.last() else {
            break;
        };
        if !is_trailing_filler(&last.to_lowercase()) {
            break;
        }
        words.pop();
    }
    words
}

/// Menggabung max_words kata pertama dan memastikan panjang tidak melebihi
/// max_chars, selalu potong di batas kata (tidak pernah memotong tengah
/// kata maupun tengah char UTF-8).
fn truncate_words(words: &[&str], max_words: usize, max_chars: usize) -> String {
    let head: Vec<&&str> = words.iter().take(max_words).collect();
    let joined: String = head
        .into_iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join(" ");
    let out = joined.trim_matches(TITLE_TRIM_CHARS).to_string();
    if out.is_empty() {
        return String::new();
    }
    if out.chars().count() <= max_chars {
        return out;
    }
    let cutoff: String = out.chars().take(max_chars).collect();
    match cutoff.rfind(' ') {
        Some(0) | None => cutoff,
        Some(i) => cutoff[..i]
            .trim_end_matches([
                ' ', '\t', '.', ',', ':', ';', '!', '?', '\u{2026}', '-', '\u{2013}', '\u{2014}',
            ])
            .to_string(),
    }
}

async fn touch_conversation(
    db: &DatabaseConnection,
    conversation: chat_conversations::Model,
) -> Result<(), ChatError> {
    let mut active = conversation.into_active_model();
    active.updated_at = Set(Utc::now().into());
    active.update(db).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_message(
    db: &DatabaseConnection,
    conversation_id: Uuid,
    user_id: Uuid,
    role: String,
    content: String,
    model: Option<String>,
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
) -> Result<chat_messages::Model, ChatError> {
    let now = Utc::now().into();
    let message = chat_messages::ActiveModel {
        conversation_id: Set(conversation_id),
        user_id: Set(user_id),
        role: Set(role),
        content: Set(content),
        model: Set(model),
        prompt_tokens: Set(Some(prompt_tokens)),
        completion_tokens: Set(Some(completion_tokens)),
        total_tokens: Set(Some(total_tokens)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(message)
}

/// All messages of a conversation in chronological order (the OpenRouter
/// context window), mirroring `GetMessagesByConversationIDAsc`.
async fn context_messages(
    db: &DatabaseConnection,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<chat_messages::Model>, ChatError> {
    let messages = chat_messages::Entity::find()
        .filter(chat_messages::Column::ConversationId.eq(conversation_id))
        .filter(chat_messages::Column::UserId.eq(user_id))
        .order_by_asc(chat_messages::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(messages)
}

/// echobackend: a conversation still titled "New conversation" after the first
/// message gets an auto-generated title from that message.
async fn maybe_auto_title(
    db: &DatabaseConnection,
    conversation: &chat_conversations::Model,
    context_len: usize,
    content: &str,
) -> Result<(), ChatError> {
    if conversation.title != "New conversation" || context_len != 1 {
        return Ok(());
    }
    let title = build_conversation_title(None, content);
    let mut active = conversation.clone().into_active_model();
    active.title = Set(title);
    active.update(db).await?;
    Ok(())
}

fn to_openrouter_messages(messages: &[chat_messages::Model]) -> Vec<OpenRouterMessage> {
    messages
        .iter()
        .map(|message| OpenRouterMessage {
            role: message.role.clone(),
            content: message.content.clone(),
        })
        .collect()
}

pub async fn create_conversation(
    db: &DatabaseConnection,
    user_id: Uuid,
    req: CreateChatConversationRequest,
) -> Result<ChatConversationResponse, ChatError> {
    let now = Utc::now().into();
    let conversation = chat_conversations::ActiveModel {
        title: Set(req.title),
        user_id: Set(user_id),
        is_pinned: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(conversation_response(conversation, None, 0))
}

pub async fn get_user_conversations(
    db: &DatabaseConnection,
    user_id: Uuid,
    offset: u64,
    limit: u64,
) -> Result<(Vec<ChatConversationResponse>, i64), ChatError> {
    let base = chat_conversations::Entity::find()
        .filter(chat_conversations::Column::UserId.eq(user_id))
        .filter(chat_conversations::Column::DeletedAt.is_null());
    let total = base.clone().count(db).await? as i64;
    let conversations = base
        .order_by_desc(chat_conversations::Column::UpdatedAt)
        .offset(offset)
        .limit(limit)
        .all(db)
        .await?;

    let mut out = Vec::with_capacity(conversations.len());
    for conversation in conversations {
        let count = chat_messages::Entity::find()
            .filter(chat_messages::Column::ConversationId.eq(conversation.id))
            .count(db)
            .await? as usize;
        out.push(conversation_response(conversation, None, count));
    }
    Ok((out, total))
}

pub async fn get_conversation_by_id(
    db: &DatabaseConnection,
    id: Uuid,
    user_id: Uuid,
) -> Result<ChatConversationResponse, ChatError> {
    let conversation = owned_conversation(db, id, user_id).await?;
    let messages = chat_messages::Entity::find()
        .filter(chat_messages::Column::ConversationId.eq(id))
        .order_by_asc(chat_messages::Column::CreatedAt)
        .all(db)
        .await?;
    let count = messages.len();
    Ok(conversation_response(conversation, Some(messages), count))
}

pub async fn update_conversation(
    db: &DatabaseConnection,
    id: Uuid,
    user_id: Uuid,
    req: UpdateChatConversationRequest,
) -> Result<ChatConversationResponse, ChatError> {
    let conversation = owned_conversation(db, id, user_id).await?;
    let mut active = conversation.into_active_model();
    if let Some(title) = req.title
        && !title.trim().is_empty()
    {
        active.title = Set(title);
    }
    if let Some(is_pinned) = req.is_pinned {
        active.is_pinned = Set(is_pinned);
        active.pinned_at = Set(if is_pinned {
            Some(Utc::now().into())
        } else {
            None
        });
    }
    active.updated_at = Set(Utc::now().into());
    let updated = active.update(db).await?;
    let count = chat_messages::Entity::find()
        .filter(chat_messages::Column::ConversationId.eq(updated.id))
        .count(db)
        .await? as usize;
    Ok(conversation_response(updated, None, count))
}

pub async fn delete_conversation(
    db: &DatabaseConnection,
    id: Uuid,
    user_id: Uuid,
) -> Result<(), ChatError> {
    let conversation = owned_conversation(db, id, user_id).await?;
    let now = Utc::now().into();
    let mut active = conversation.into_active_model();
    active.deleted_at = Set(Some(now));
    active.updated_at = Set(now);
    active.update(db).await?;
    Ok(())
}

/// Stores the user message, then (when the role is `user` and OpenRouter is
/// configured) synchronously generates and stores the AI reply, mirroring
/// echobackend's `CreateMessage`. AI failures are fail-open: the user message
/// is still returned on its own.
pub async fn create_message(
    db: &DatabaseConnection,
    user_id: Uuid,
    conversation_id: Uuid,
    req: CreateChatMessageRequest,
) -> Result<Vec<ChatMessageResponse>, ChatError> {
    let conversation = owned_conversation(db, conversation_id, user_id).await?;
    let role = normalized_role(req.role.clone());
    let content = req.content.clone();
    let user_message = insert_message(
        db,
        conversation_id,
        user_id,
        role.clone(),
        content.clone(),
        req.model.clone(),
        0,
        0,
        0,
    )
    .await?;
    touch_conversation(db, conversation.clone()).await?;
    let mut responses = vec![message_response(user_message)];

    if role != "user" || !openrouter::is_available(req.model.as_deref()) {
        return Ok(responses);
    }

    let context = context_messages(db, conversation_id, user_id).await?;
    maybe_auto_title(db, &conversation, context.len(), &content).await?;

    let messages = to_openrouter_messages(&context);
    let reply = match openrouter::generate_response(
        &messages,
        req.model.as_deref(),
        openrouter::normalized_temperature(req.temperature),
    )
    .await
    {
        Ok(reply) => reply,
        Err(err) => {
            tracing::warn!(error = %err, "openrouter generate response failed; returning user message only");
            return Ok(responses);
        }
    };
    let Some(choice) = reply.choices.into_iter().next() else {
        return Ok(responses);
    };

    let assistant = insert_message(
        db,
        conversation_id,
        user_id,
        normalized_role(Some(choice.message.role)),
        choice.message.content,
        openrouter::effective_model(req.model.as_deref()),
        reply.usage.prompt_tokens,
        reply.usage.completion_tokens,
        reply.usage.total_tokens,
    )
    .await?;
    touch_conversation(db, conversation).await?;
    responses.push(message_response(assistant));
    Ok(responses)
}

/// `POST /api/chat/conversations/stream`: creates a conversation, stores the
/// first message, and prepares the AI stream (or the fallback), mirroring
/// echobackend's `CreateConversationStream`.
pub async fn create_conversation_stream(
    db: &DatabaseConnection,
    user_id: Uuid,
    req: CreateChatConversationStreamRequest,
) -> Result<StreamOutcome, ChatError> {
    let title = build_conversation_title(req.title.clone(), &req.content);
    let now = Utc::now().into();
    let conversation = chat_conversations::ActiveModel {
        title: Set(title),
        user_id: Set(user_id),
        is_pinned: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    prepare_streaming_message(
        db,
        user_id,
        conversation.id,
        CreateChatMessageRequest {
            content: req.content,
            role: Some("user".to_string()),
            model: req.model,
            temperature: req.temperature,
        },
    )
    .await
}

/// Stores the user message and decides whether the AI stream can run,
/// mirroring echobackend's `createStreamingMessageInternal`.
pub async fn prepare_streaming_message(
    db: &DatabaseConnection,
    user_id: Uuid,
    conversation_id: Uuid,
    req: CreateChatMessageRequest,
) -> Result<StreamOutcome, ChatError> {
    let conversation = owned_conversation(db, conversation_id, user_id).await?;
    let role = normalized_role(req.role.clone());
    let content = req.content.clone();
    let user_message = insert_message(
        db,
        conversation_id,
        user_id,
        role.clone(),
        content.clone(),
        req.model.clone(),
        0,
        0,
        0,
    )
    .await?;
    touch_conversation(db, conversation.clone()).await?;
    let user_response = message_response(user_message);

    if role != "user" || !openrouter::is_available(req.model.as_deref()) {
        return Ok(StreamOutcome::Fallback(user_response));
    }

    let context = context_messages(db, conversation_id, user_id).await?;
    maybe_auto_title(db, &conversation, context.len(), &content).await?;

    Ok(StreamOutcome::Stream(StreamPreparation {
        user_id,
        conversation_id,
        user_message: user_response,
        context_messages: to_openrouter_messages(&context),
        model: req.model,
        temperature: openrouter::normalized_temperature(req.temperature),
    }))
}

/// Persists the completed streamed AI message, mirroring echobackend's
/// `SaveStreamingMessage`.
pub async fn save_streaming_message(
    db: &DatabaseConnection,
    conversation_id: Uuid,
    user_id: Uuid,
    content: String,
    model: Option<String>,
    usage: OpenRouterUsage,
) -> Result<ChatMessageResponse, ChatError> {
    let conversation = owned_conversation(db, conversation_id, user_id).await?;
    let message = insert_message(
        db,
        conversation_id,
        user_id,
        "assistant".to_string(),
        content,
        openrouter::effective_model(model.as_deref()),
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.total_tokens,
    )
    .await?;
    touch_conversation(db, conversation).await?;
    Ok(message_response(message))
}

pub async fn get_messages(
    db: &DatabaseConnection,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<ChatMessageResponse>, ChatError> {
    owned_conversation(db, conversation_id, user_id).await?;
    let messages = chat_messages::Entity::find()
        .filter(chat_messages::Column::ConversationId.eq(conversation_id))
        .order_by_desc(chat_messages::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(messages.into_iter().map(message_response).collect())
}

pub async fn get_message(
    db: &DatabaseConnection,
    message_id: Uuid,
    user_id: Uuid,
) -> Result<ChatMessageResponse, ChatError> {
    let message = chat_messages::Entity::find_by_id(message_id)
        .filter(chat_messages::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .ok_or(ChatError::MessageNotFound)?;
    owned_conversation(db, message.conversation_id, user_id).await?;
    Ok(message_response(message))
}

pub async fn delete_message(
    db: &DatabaseConnection,
    message_id: Uuid,
    user_id: Uuid,
) -> Result<ChatMessageResponse, ChatError> {
    let message = chat_messages::Entity::find_by_id(message_id)
        .filter(chat_messages::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .ok_or(ChatError::MessageNotFound)?;
    let conversation = owned_conversation(db, message.conversation_id, user_id).await?;
    chat_messages::Entity::delete_by_id(message_id)
        .exec(db)
        .await?;
    touch_conversation(db, conversation).await?;
    Ok(message_response(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(content: &str) -> String {
        build_conversation_title(None, content)
    }

    #[test]
    fn explicit_title_is_respected() {
        let got = build_conversation_title(Some("  Laporan Q1  ".to_string()), "abaikan ini");
        assert_eq!(got, "Laporan Q1");
    }

    #[test]
    fn empty_content_falls_back() {
        assert_eq!(title("   "), "New conversation");
    }

    #[test]
    fn strips_leading_filler_words() {
        assert_eq!(
            title("tolong buatkan laporan keuangan bulan januari untuk presentasi besok"),
            "laporan keuangan bulan januari untuk presentasi besok"
        );
    }

    #[test]
    fn takes_first_sentence() {
        assert_eq!(
            title("Apa itu inflasi? Jelaskan dampaknya ke pasar saham Indonesia secara detail"),
            "inflasi"
        );
    }

    #[test]
    fn truncates_at_word_boundary() {
        assert_eq!(
            title(
                "analisis perbandingan saham bank BCA BRI Mandiri BNI BTN CIMB Danamon Permata OCBC tahun ini"
            ),
            "analisis perbandingan saham bank BCA BRI Mandiri BNI"
        );
    }

    #[test]
    fn never_splits_utf8_char() {
        let got = title(
            "ceritakan tentang café crème brûlée naïve façade di kota Zürich yang indah sekali dan menawan hati",
        );
        assert_eq!(got, "ceritakan tentang café crème brûlée naïve façade di");
        assert!(got.chars().count() <= MAX_TITLE_CHARS);
    }

    #[test]
    fn strips_markdown_and_list_prefix() {
        assert_eq!(
            title("### 1. Jelaskan strategi investasi untuk pemula yang baru mulai"),
            "strategi investasi untuk pemula yang baru mulai"
        );
    }

    #[test]
    fn strips_trailing_filler() {
        assert_eq!(
            title("jelaskan bedanya saham dan obligasi untuk pemula ya"),
            "bedanya saham dan obligasi untuk pemula"
        );
    }

    #[test]
    fn explicit_long_title_truncated_to_db_limit() {
        let long = "a".repeat(300);
        let got = build_conversation_title(Some(long), "fallback");
        assert_eq!(got.chars().count(), MAX_EXPLICIT_TITLE_CHARS);
    }
}
