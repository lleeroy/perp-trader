use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Balance information from exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub free: Decimal,
    pub locked: Decimal,
}

impl Balance {
    pub fn total(&self) -> Decimal {
        self.free + self.locked
    }
}