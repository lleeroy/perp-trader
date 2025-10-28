pub mod encode;

use chrono::{DateTime, Utc};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use serde::Deserialize;


pub fn deserialize_decimal_from_string<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => Decimal::from_str_exact(&s).map_err(D::Error::custom),
        serde_json::Value::Number(n) => {
            Decimal::from_f64(n.as_f64().unwrap()).ok_or_else(|| D::Error::custom("Invalid number"))
        }
        other => Err(D::Error::custom(format!(
            "unexpected type for Decimal: {:?}",
            other
        ))),
    }
}

pub fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    use chrono::{TimeZone, Utc};

    // Handles either String or integer timestamps
    struct TsVisitor;
    impl<'de> serde::de::Visitor<'de> for TsVisitor {
        type Value = DateTime<Utc>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a unix timestamp as a string or integer")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse::<i64>()
                .map_err(E::custom)
                .and_then(|ts| Utc.timestamp_opt(ts, 0).single().ok_or_else(|| E::custom("Invalid timestamp")))
        }
        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            self.visit_str(&v)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Utc.timestamp_opt(v as i64, 0).single().ok_or_else(|| E::custom("Invalid timestamp"))
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Utc.timestamp_opt(v, 0).single().ok_or_else(|| E::custom("Invalid timestamp"))
        }
    }
    deserializer.deserialize_any(TsVisitor)
}