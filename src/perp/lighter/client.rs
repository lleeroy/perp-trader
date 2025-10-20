use async_trait::async_trait;
use lighter_rust::{LighterClient as LighterRustClient, Config};
use rust_decimal::Decimal;
use crate::{error::TradingError, model::{balance::Balance, token::Token, Position, PositionSide}, perp::PerpExchange, trader::wallet::Wallet};

pub struct LighterClient {
    client: LighterRustClient
}

impl LighterClient {
    /// Creates a new `LighterClient` instance using the provided wallet credentials.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Reference to the user's wallet struct containing API secrets for authentication.
    ///
    /// # Returns
    ///
    /// * `LighterClient` - A client instance ready to communicate with the Lighter API.
    pub fn new(wallet: &Wallet) -> Result<Self, TradingError> {
        let config = Config::new().with_api_key(&wallet.lighter_api_key);
        let client = LighterRustClient::new(config, &wallet.private_key)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        Ok(Self { client })
    }

    pub async fn is_authenticated(&self) -> bool {
        self.client.account().get_account().await.is_ok()
    }
}

#[async_trait]
impl PerpExchange for LighterClient {
    fn name(&self) -> &str {
        "Lighter"
    }

    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }

    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError> {
        // TODO: Implement actual API call
        log::warn!("Hibachi get_balance not fully implemented for {}", asset);
        Ok(Balance {
            asset: asset.to_string(),
            free: Decimal::from(1000),
            locked: Decimal::ZERO,
        })
    }

    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError> {
        // TODO: Implement actual API call
        log::warn!("Hibachi get_balances not fully implemented");
        Ok(vec![])
    }

    async fn open_position(&self, token: Token, side: PositionSide, amount_usdc: Decimal) -> Result<Position, TradingError> {
        // TODO: Implement actual API call
        todo!("Hibachi open_position not fully implemented for {}", side);
    }


    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        // TODO: Implement actual API call
        log::warn!("Hibachi get_usdc_balance not fully implemented");
        Ok(Decimal::ZERO)
    }

    async fn close_position(&self, position: &Position) -> Result<Position, TradingError> {
        // TODO: Implement actual API call
        todo!("Hibachi close_position not fully implemented for {}", position.side);
    }
}

