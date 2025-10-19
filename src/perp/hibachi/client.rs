#![allow(unused)]

use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::config::ExchangeCredentials;
use crate::error::TradingError;
use crate::model::{Balance};
use crate::perp::PerpExchange;
use crate::trader::wallet::Wallet;

/// Hibachi exchange client
pub struct HibachiClient {
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl HibachiClient {
    pub fn new(wallet: &Wallet) -> Self {
        let base_url = "https://api.hibachi.exchange";

        Self {
            api_key: wallet.hibachi_api_key.clone(),
            api_secret: wallet.hibachi_api_secret.clone(),
            base_url: base_url.to_string(),
        }
    }
}

#[async_trait]
impl PerpExchange for HibachiClient {
    fn name(&self) -> &str {
        "Hibachi"
    }

    async fn health_check(&self) -> Result<bool, TradingError> {
        // TODO: Implement actual health check
        // For now, just return true if credentials are set
        Ok(!self.api_key.is_empty() && !self.api_secret.is_empty())
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
}

