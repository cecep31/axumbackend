use crate::models::report::{EngagementMetricsResponse, OverviewStatsResponse};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ReportQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    /// Kept as a raw string and parsed leniently (mirrors echobackend's
    /// `strconv.Atoi`, which silently falls back to the default instead of
    /// rejecting the request for a non-numeric/out-of-range `limit`).
    pub limit: Option<String>,
    /// Kept as a raw string and parsed leniently (mirrors echobackend's
    /// `strconv.Atoi`, which silently ignores a malformed `tagId` instead of
    /// rejecting the request).
    pub tag_id: Option<String>,
}

impl ReportQuery {
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

#[derive(Serialize)]
pub struct OverviewReport {
    pub overview: OverviewStatsResponse,
    pub engagement: EngagementMetricsResponse,
}

pub fn date_range(query: &ReportQuery) -> crate::services::report::DateRange<'_> {
    crate::services::report::DateRange {
        start_date: query.start_date.as_deref(),
        end_date: query.end_date.as_deref(),
    }
}
