use async_trait::async_trait;
use crate::config::{AppConfig};
use crate::error::TradingError;
use crate::model::{Balance};
use crate::perp::PerpExchange;
use crate::trader::wallet::Wallet;
use bpx_api_client::{BACKPACK_API_BASE_URL, BpxClient};


/// Backpack exchange client
pub struct BackpackClient {
    client: BpxClient,
}

impl BackpackClient {
    pub fn new(wallet: &Wallet) -> Self {
        let client = BpxClient::init(
            BACKPACK_API_BASE_URL.to_string(),
            &wallet.backpack_api_key, 
            None
        ).unwrap();

        Self {
            client,
        }
    }

    pub async fn is_authenticated(&self) -> bool {
        self.client.get_account().await.is_ok()
    }
}

#[async_trait]
impl PerpExchange for BackpackClient {
    fn name(&self) -> &str {
        "Backpack"
    }

    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }

    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError> {
        let balances = self.client.get_balances().await.map_err(|e| TradingError::ExchangeError(e.to_string()))?;
        let balance = balances.iter().find(|b| b.0 == asset).ok_or(TradingError::ExchangeError(format!("Asset {} not found", asset)))?;
        
        Ok(Balance {
            asset: balance.0.clone(),
            free: balance.1.available,
            locked: balance.1.locked,
        })
    }

    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError> {
        let balances = self.client.get_balances().await.map_err(|e| TradingError::ExchangeError(e.to_string()))?;
        Ok(balances.iter().map(|b| Balance {
            asset: b.0.clone(),
            free: b.1.available,
            locked: b.1.locked,
        }).collect())
    }
}
