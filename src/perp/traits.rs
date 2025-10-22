#![allow(unused)]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use crate::error::TradingError;
use crate::model::token::Token;
use crate::model::{balance::Balance, position::{Position, PositionSide}};

/// Trait for perpetual futures exchange operations
#[async_trait]
pub trait PerpExchange: Send + Sync {
    /// Get the exchange name
    fn name(&self) -> &str;

    /// Check if the exchange is available and responsive
    async fn health_check(&self) -> Result<bool, TradingError>;

    /// Get account balance for a specific asset
    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError>;

    /// Open a new position on the exchange
    async fn open_position(&self, token: Token, side: PositionSide, close_at: DateTime<Utc>, amount_usdc: Decimal) -> Result<Position, TradingError>;

    /// Close a position on the exchange
    async fn close_position(&self, position: &Position) -> Result<Position, TradingError>;

    /// Close all positions on the exchange
    async fn close_all_positions(&self) -> Result<(), TradingError>;

    /// Get the USDC balance for the account
    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError>;
}

