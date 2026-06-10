use crate::dto::chat::{
    CreateChatConversationRequest, CreateChatConversationStreamRequest, CreateChatMessageRequest,
    UpdateChatConversationRequest,
};
use crate::entities::{chat_conversations, chat_messages};
use crate::models::chat::{
    ChatConversationResponse, ChatMessageResponse, ChatStreamResult, conversation_response,
    message_response,
};
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
    MessageNotFound,
}

impl From<DbErr> for ChatError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

async fn owned_conversation(
    db: &DatabaseConnection,
    id: Uuid,
    user_id: Uuid,
) -> Result<chat_conversations::Model, ChatError> {
    chat_conversations::Entity::find_by_id(id)
        .filter(chat_conversations::Column::UserId.eq(user_id))
        .filter(chat_conversations::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(ChatError::ConversationNotFound)
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
        .order_by_desc(chat_conversations::Column::IsPinned)
        .order_by_desc(chat_conversations::Column::PinnedAt)
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

pub async fn create_message(
    db: &DatabaseConnection,
    user_id: Uuid,
    conversation_id: Uuid,
    req: CreateChatMessageRequest,
) -> Result<Vec<ChatMessageResponse>, ChatError> {
    let conversation = owned_conversation(db, conversation_id, user_id).await?;
    let now = Utc::now().into();
    let message = chat_messages::ActiveModel {
        conversation_id: Set(conversation_id),
        user_id: Set(user_id),
        role: Set(req.role.unwrap_or_else(|| "user".to_string())),
        content: Set(req.content),
        model: Set(req.model),
        prompt_tokens: Set(Some(0)),
        completion_tokens: Set(Some(0)),
        total_tokens: Set(Some(0)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    touch_conversation(db, conversation).await?;
    Ok(vec![message_response(message)])
}

pub async fn create_conversation_message(
    db: &DatabaseConnection,
    user_id: Uuid,
    req: CreateChatConversationStreamRequest,
) -> Result<ChatStreamResult, ChatError> {
    let title = req
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| req.content.chars().take(80).collect());
    let conversation =
        create_conversation(db, user_id, CreateChatConversationRequest { title }).await?;
    let conversation_id =
        Uuid::parse_str(&conversation.id).map_err(|_| ChatError::ConversationNotFound)?;
    let messages = create_message(
        db,
        user_id,
        conversation_id,
        CreateChatMessageRequest {
            content: req.content,
            role: Some("user".to_string()),
            model: req.model,
            temperature: req.temperature,
        },
    )
    .await?;
    Ok(ChatStreamResult {
        user_message: messages
            .into_iter()
            .next()
            .ok_or(ChatError::MessageNotFound)?,
        conversation_id: Some(conversation.id),
    })
}

pub async fn get_messages(
    db: &DatabaseConnection,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<ChatMessageResponse>, ChatError> {
    owned_conversation(db, conversation_id, user_id).await?;
    let messages = chat_messages::Entity::find()
        .filter(chat_messages::Column::ConversationId.eq(conversation_id))
        .order_by_asc(chat_messages::Column::CreatedAt)
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
