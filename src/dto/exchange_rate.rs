use garde::Validate;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize, Validate)]
pub struct ExchangeRateQuery {
    #[serde(default)]
    #[garde(skip)]
    pub from: String,
    #[serde(default)]
    #[garde(skip)]
    pub to: String,
}
