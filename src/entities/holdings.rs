use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "holdings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user_id: Uuid,
    pub name: String,
    pub symbol: Option<String>,
    pub platform: String,
    pub holding_type_id: i16,
    pub currency: String,
    pub invested_amount: Decimal,
    pub current_value: Decimal,
    pub gain_amount: Option<Decimal>,
    pub gain_percent: Option<Decimal>,
    pub units: Option<Decimal>,
    pub avg_buy_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub last_updated: Option<DateTimeWithTimeZone>,
    pub notes: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub month: i32,
    pub year: i32,
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
    #[sea_orm(
        belongs_to = "super::holding_types::Entity",
        from = "Column::HoldingTypeId",
        to = "super::holding_types::Column::Id",
        on_update = "NoAction",
        on_delete = "Restrict"
    )]
    HoldingTypes,
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl Related<super::holding_types::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::HoldingTypes.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
