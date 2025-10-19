#![allow(unused)]

use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::config::ExchangeCredentials;
use crate::error::TradingError;
use crate::model::{Balance};
use crate::perp::PerpExchange;

/// Hibachi exchange client
pub struct HibachiClient {
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl HibachiClient {
    pub fn new(credentials: &ExchangeCredentials) -> Self {
        let base_url = credentials
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.hibachi.exchange".to_string());

        Self {
            api_key: credentials.api_key.clone(),
            api_secret: credentials.api_secret.clone(),
            base_url,
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

