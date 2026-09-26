use sea_orm::entity::prelude::*;

/// One link in a refresh-token rotation chain, mirroring echobackend's
/// `model.Session`.
///
/// Every token minted from a single login shares a `family_id`. Refreshing
/// stamps the current row with `rotated_at`/`replaced_by` and inserts a
/// successor; rotated rows are kept until they expire so a replayed token can
/// still be recognised and its family revoked (RFC 9700 §4.14.2).
///
/// `refresh_token` stores the SHA-256 hex digest of the token, never the token
/// itself.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    #[sea_orm(unique)]
    pub refresh_token: String,
    pub user_id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    /// Sliding inactivity deadline, pushed forward by each rotation.
    pub expires_at: DateTimeWithTimeZone,
    /// Caps the whole family regardless of activity; copied unchanged from the
    /// root session, so rotation can never extend it.
    pub absolute_expires_at: DateTimeWithTimeZone,
    /// Set once this token has been exchanged for a successor.
    pub rotated_at: Option<DateTimeWithTimeZone>,
    /// Hashed successor token, kept for audit only.
    pub replaced_by: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::UserId",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Users,
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
