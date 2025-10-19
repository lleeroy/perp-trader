#![allow(unused)]

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::exchange::Exchange;

/// Position side (Long or Short)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionSide {
    Long,
    Short,
}

impl std::fmt::Display for PositionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionSide::Long => write!(f, "LONG"),
            PositionSide::Short => write!(f, "SHORT"),
        }
    }
}

impl std::str::FromStr for PositionSide {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "LONG" => Ok(PositionSide::Long),
            "SHORT" => Ok(PositionSide::Short),
            _ => Err(format!("Invalid PositionSide: {}", s)),
        }
    }
}

/// Position status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    /// Position is currently open
    Open,
    /// Position is being closed
    Closing,
    /// Position has been closed
    Closed,
    /// Position opening failed
    Failed,
}

impl std::fmt::Display for PositionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionStatus::Open => write!(f, "OPEN"),
            PositionStatus::Closing => write!(f, "CLOSING"),
            PositionStatus::Closed => write!(f, "CLOSED"),
            PositionStatus::Failed => write!(f, "FAILED"),
        }
    }
}

impl std::str::FromStr for PositionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "OPEN" => Ok(PositionStatus::Open),
            "CLOSING" => Ok(PositionStatus::Closing),
            "CLOSED" => Ok(PositionStatus::Closed),
            "FAILED" => Ok(PositionStatus::Failed),
            _ => Err(format!("Invalid PositionStatus: {}", s)),
        }
    }
}

/// Individual position on a single exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Unique identifier for this position
    pub id: String,
    pub exchange: Exchange,
    /// Trading pair symbol (e.g., "BTC-PERP")
    pub symbol: String,
    /// Position side (Long or Short)
    pub side: PositionSide,
    /// Position size (in base currency)
    pub size: Decimal,
    /// Current position status
    pub status: PositionStatus,
    /// When the position was opened
    pub opened_at: DateTime<Utc>,
    /// When the position should be closed
    pub close_at: DateTime<Utc>,
    /// When the position was actually closed
    pub closed_at: Option<DateTime<Utc>>,
    /// Realized PnL when closed
    pub realized_pnl: Option<Decimal>,
    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}
