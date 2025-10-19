use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::error::TradingError;
use crate::model::{Balance, MarketData, OpenPositionRequest, OpenPositionResponse, PositionInfo};

/// Trait for perpetual futures exchange operations
#[async_trait]
pub trait PerpExchange: Send + Sync {
    /// Get the exchange name
    fn name(&self) -> &str;

    /// Check if the exchange is available and responsive
    async fn health_check(&self) -> Result<bool, TradingError>;

    /// Get account balance for a specific asset
    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError>;

    /// Get all account balances
    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError>;

    /// Get current market data for a symbol
    async fn get_market_data(&self, symbol: &str) -> Result<MarketData, TradingError>;

    /// Get current open positions
    async fn get_positions(&self) -> Result<Vec<PositionInfo>, TradingError>;

    /// Get a specific position by symbol
    async fn get_position(&self, symbol: &str) -> Result<Option<PositionInfo>, TradingError>;

    /// Open a new position
    async fn open_position(
        &self,
        request: OpenPositionRequest,
    ) -> Result<OpenPositionResponse, TradingError>;

    /// Close a position
    async fn close_position(
        &self,
        symbol: &str,
        size: Option<Decimal>,
    ) -> Result<(), TradingError>;

    /// Set leverage for a symbol
    async fn set_leverage(&self, symbol: &str, leverage: Decimal) -> Result<(), TradingError>;

    /// Get minimum order size for a symbol
    async fn get_min_order_size(&self, symbol: &str) -> Result<Decimal, TradingError>;

    /// Validate if we have sufficient collateral for a position
    async fn validate_collateral(
        &self,
        symbol: &str,
        size: Decimal,
        leverage: Decimal,
    ) -> Result<bool, TradingError> {
        let market_data = self.get_market_data(symbol).await?;
        let required_collateral = (size * market_data.mark_price) / leverage;

        // Assuming USDT as collateral
        let balance = self.get_balance("USDT").await?;
        
        Ok(balance.free >= required_collateral)
    }
}

