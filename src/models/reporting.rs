use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ReportQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub interval: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub location_id: Option<i64>,
    pub variant_id: Option<i64>,
    pub supplier_id: Option<i64>,
    pub status: Option<String>,
    pub threshold: Option<String>,
    pub payment_method: Option<String>,
}
