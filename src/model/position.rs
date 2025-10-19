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
    /// ID of the hedge pair this position belongs to
    pub hedge_pair_id: String,
    /// Exchange where this position is opened
    pub exchange: Exchange,
    /// Trading pair symbol (e.g., "BTC-PERP")
    pub symbol: String,
    /// Position side (Long or Short)
    pub side: PositionSide,
    /// Leverage multiplier
    pub leverage: Decimal,
    /// Position size (in base currency)
    pub size: Decimal,
    /// Entry price
    pub entry_price: Decimal,
    /// Current price (updated during monitoring)
    pub current_price: Option<Decimal>,
    /// Collateral amount
    pub collateral: Decimal,
    /// Current position status
    pub status: PositionStatus,
    /// Exchange-specific position ID
    pub exchange_position_id: Option<String>,
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

/// A pair of hedged positions (one long, one short)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgePair {
    /// Unique identifier for this hedge pair
    pub id: String,
    /// Trading symbol
    pub symbol: String,
    /// Status of the hedge pair
    pub status: PositionStatus,
    /// Leverage used for both positions
    pub leverage: Decimal,
    /// Position size
    pub size: Decimal,
    /// When the pair was created
    pub created_at: DateTime<Utc>,
    /// When the pair should be closed
    pub close_at: DateTime<Utc>,
    /// When the pair was closed
    pub closed_at: Option<DateTime<Utc>>,
    /// Combined realized PnL
    pub total_realized_pnl: Option<Decimal>,
    /// Points earned (if applicable)
    pub points_earned: Option<Decimal>,
}

/// Real-time position information from exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub symbol: String,
    pub side: PositionSide,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub mark_price: Decimal,
    pub liquidation_price: Option<Decimal>,
    pub unrealized_pnl: Decimal,
    pub collateral: Decimal,
    pub leverage: Decimal,
}

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

/// Market data for a symbol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketData {
    pub symbol: String,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub funding_rate: Option<Decimal>,
    pub open_interest: Option<Decimal>,
    pub volume_24h: Option<Decimal>,
}

/// Request to open a new position
#[derive(Debug, Clone)]
pub struct OpenPositionRequest {
    pub symbol: String,
    pub side: PositionSide,
    pub size: Decimal,
    pub leverage: Decimal,
    pub reduce_only: bool,
}

/// Response after opening a position
#[derive(Debug, Clone)]
pub struct OpenPositionResponse {
    pub position_id: String,
    pub symbol: String,
    pub side: PositionSide,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub leverage: Decimal,
}

impl Position {
    /// Create a new position
    pub fn new(
        hedge_pair_id: String,
        exchange: Exchange,
        symbol: String,
        side: PositionSide,
        leverage: Decimal,
        size: Decimal,
        entry_price: Decimal,
        collateral: Decimal,
        close_at: DateTime<Utc>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            hedge_pair_id,
            exchange,
            symbol,
            side,
            leverage,
            size,
            entry_price,
            current_price: Some(entry_price),
            collateral,
            status: PositionStatus::Open,
            exchange_position_id: None,
            opened_at: now,
            close_at,
            closed_at: None,
            realized_pnl: None,
            updated_at: now,
        }
    }

    /// Calculate unrealized PnL
    pub fn unrealized_pnl(&self) -> Option<Decimal> {
        self.current_price.map(|current| {
            let price_diff = match self.side {
                PositionSide::Long => current - self.entry_price,
                PositionSide::Short => self.entry_price - current,
            };
            price_diff * self.size
        })
    }

    /// Calculate current collateral ratio
    pub fn collateral_ratio(&self) -> Option<Decimal> {
        self.unrealized_pnl().map(|pnl| {
            let equity = self.collateral + pnl;
            let position_value = self.size * self.entry_price;
            if position_value.is_zero() {
                Decimal::MAX
            } else {
                equity / position_value * self.leverage
            }
        })
    }

    /// Check if position is at risk of liquidation
    pub fn is_at_risk(&self, min_ratio: Decimal) -> bool {
        self.collateral_ratio()
            .map(|ratio| ratio < min_ratio)
            .unwrap_or(false)
    }
}

impl HedgePair {
    /// Create a new hedge pair
    pub fn new(symbol: String, leverage: Decimal, size: Decimal, close_at: DateTime<Utc>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            symbol,
            status: PositionStatus::Open,
            leverage,
            size,
            created_at: now,
            close_at,
            closed_at: None,
            total_realized_pnl: None,
            points_earned: None,
        }
    }
}

