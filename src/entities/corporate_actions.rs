use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "corporate_actions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub symbol: String,
    pub name: Option<String>,
    pub r#type: String,
    pub event_date: Date,
    pub pay_date: Option<Date>,
    pub amount: Option<Decimal>,
    pub currency: Option<String>,
    pub note: Option<String>,
    pub market: String,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
