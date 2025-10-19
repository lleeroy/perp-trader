use async_trait::async_trait;
use crate::error::TradingError;
use crate::model::{Balance};

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
}

