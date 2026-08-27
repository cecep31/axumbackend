use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "holding_types")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i16,
    pub code: String,
    pub name: String,
    pub notes: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::holdings::Entity")]
    Holdings,
}

impl Related<super::holdings::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Holdings.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
