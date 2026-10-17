use crate::services::stats::DateRange;
use garde::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct StatsQuery {
    #[garde(skip)]
    pub start_date: Option<String>,
    #[garde(skip)]
    pub end_date: Option<String>,
    /// Kept as a raw string and parsed leniently (mirrors echobackend's
    /// `strconv.Atoi`, which silently falls back to the default instead of
    /// rejecting the request for a non-numeric/out-of-range `limit`).
    #[garde(skip)]
    pub limit: Option<String>,
    /// Kept as a raw string and parsed leniently (mirrors echobackend's
    /// `strconv.Atoi`, which silently ignores a malformed `tagId` instead of
    /// rejecting the request).
    #[garde(skip)]
    pub tag_id: Option<String>,
}

impl StatsQuery {
    pub fn date_range(&self) -> DateRange<'_> {
        DateRange {
            start_date: self.start_date.as_deref(),
            end_date: self.end_date.as_deref(),
        }
    }

    pub fn tag_id(&self) -> Option<i32> {
        self.tag_id.as_deref()?.parse().ok()
    }

    /// Mirrors echobackend's inline `limit` parsing in `GetUsers`/`GetPosts`:
    /// non-numeric or `<= 0` falls back to `10`; clamped to a max of `100`.
    pub fn limit(&self) -> i64 {
        let mut limit = self
            .limit
            .as_deref()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if limit <= 0 {
            limit = 10;
        }
        if limit > 100 {
            limit = 100;
        }
        limit
    }
}
