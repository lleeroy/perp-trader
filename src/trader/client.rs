use crate::{
    error::TradingError, 
    model::{token::Token, position::{Position, PositionStatus}}, 
    trader::{wallet::Wallet, storage::PositionStorage}
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

pub struct TraderClient {
    wallet: Wallet,
    storage: PositionStorage,
}

impl TraderClient {
    /// Create a new trader client with default database location
    pub fn new_by_wallet_id(wallet_id: u8) -> Result<Self, TradingError> {
        let wallet = Wallet::load_from_json(wallet_id)?;
        let storage = PositionStorage::new("positions.db")?;

        Ok(Self { wallet, storage })
    }

    pub async fn farm_points_on_backpack_from_multiple_wallets(wallets: Vec<Wallet>) -> Result<(), TradingError> {
        if wallets.is_empty() {
            return Err(TradingError::InvalidInput("No wallets provided".into()));
        }

        if wallets.len() < 3 {
            return Err(TradingError::InvalidInput("At least 3 wallets are required".into()));
        }

        Ok(())
    }

    pub fn get_supported_tokens(&self) -> Vec<Token> {
        Token::get_supported_tokens()
    }

    // ===== Position Storage Methods =====

    /// Save or update a position in storage
    pub fn save_position(&self, position: &Position) -> Result<(), TradingError> {
        self.storage.save_position(position)
    }

    /// Get a specific position by ID
    pub fn get_position(&self, id: &str) -> Result<Option<Position>, TradingError> {
        self.storage.get_position(id)
    }

    /// Get all stored positions
    pub fn get_all_positions(&self) -> Result<Vec<Position>, TradingError> {
        self.storage.get_all_positions()
    }

    /// Get positions for a specific exchange
    pub fn get_positions_by_exchange(&self, exchange: crate::model::exchange::Exchange) -> Result<Vec<Position>, TradingError> {
        self.storage.get_positions_by_exchange(exchange)
    }

    /// Get all active (Open or Closing) positions
    pub fn get_active_positions(&self) -> Result<Vec<Position>, TradingError> {
        self.storage.get_active_positions()
    }

    /// Update a position's status and related fields
    pub fn update_position_status(
        &self,
        id: &str,
        status: PositionStatus,
        closed_at: Option<DateTime<Utc>>,
        realized_pnl: Option<Decimal>,
    ) -> Result<(), TradingError> {
        self.storage.update_position_status(id, status, closed_at, realized_pnl)
    }

    /// Delete a position from storage (use sparingly)
    pub fn delete_position(&self, id: &str) -> Result<(), TradingError> {
        self.storage.delete_position(id)
    }
}