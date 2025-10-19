use async_trait::async_trait;
use crate::error::TradingError;
use crate::model::{Balance};
use crate::perp::PerpExchange;
use crate::trader::wallet::Wallet;
use bpx_api_client::{BACKPACK_API_BASE_URL, BpxClient};

/// Backpack exchange client for interacting with the Backpack perpetual futures exchange.
pub struct BackpackClient {
    client: BpxClient,
}

impl BackpackClient {
    /// Creates a new `BackpackClient` instance using the provided wallet credentials.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Reference to the user's wallet struct containing API secrets for authentication.
    ///
    /// # Returns
    ///
    /// * `BackpackClient` - A client instance ready to communicate with the Backpack API.
    pub fn new(wallet: &Wallet) -> Self {
        let client = BpxClient::init(
            BACKPACK_API_BASE_URL.to_string(),
            &wallet.backpack_api_secret, 
            None
        ).unwrap();

        Self {
            client,
        }
    }

    /// Checks if the client is currently authenticated with the Backpack API.
    ///
    /// # Returns
    ///
    /// * `bool` - Returns `true` if authentication is successful, else `false`.
    pub async fn is_authenticated(&self) -> bool {
        self.client.get_account().await.is_ok()
    }
}

#[async_trait]
impl PerpExchange for BackpackClient {
    /// Returns the name of the exchange ("Backpack").
    fn name(&self) -> &str {
        "Backpack"
    }

    /// Checks the health of the Backpack exchange client.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` if authentication check passes, otherwise `Ok(false)` or an error.
    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }

    /// Fetches the balance for a specific asset from the Backpack exchange.
    ///
    /// # Arguments
    ///
    /// * `asset` - The asset symbol (e.g., "USDC") for which to fetch the balance.
    ///
    /// # Returns
    ///
    /// * `Ok(Balance)` containing free and locked balances for the asset, or a `TradingError`.
    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError> {
        let balances = self.client.get_balances()
            .await.map_err(|e| TradingError::ExchangeError(e.to_string()))?;

        let balance = balances.iter().find(|b| 
            b.0.to_lowercase() == asset.to_lowercase())
            .ok_or(TradingError::ExchangeError(format!("Asset {} not found", asset)))?;
        
        Ok(Balance {
            asset: balance.0.clone(),
            free: balance.1.available,
            locked: balance.1.locked,
        })
    }

    /// Fetches all available balances from the Backpack exchange.
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<Balance>)` - A vector of balances for all supported assets, or a `TradingError`.
    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError> {
        let balances = self.client.get_balances()
            .await.map_err(|e| TradingError::ExchangeError(e.to_string()))?;
        
        Ok(balances.iter().map(|b| Balance {
            asset: b.0.clone(),
            free: b.1.available,
            locked: b.1.locked,
        }).collect())
    }
}
