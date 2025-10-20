use std::{fs::File, io::BufReader};
use serde::{Deserialize, Serialize};
use anyhow::{Result};

use crate::{config::AppConfig, error::TradingError, helpers::encode};

/// Wallet struct containing API secrets for authentication with exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(unused)]
pub struct Wallet {
    pub id: u8,
    pub private_key: String,
    pub backpack_api_key: String,
    pub backpack_api_secret: String,
    pub hibachi_api_key: String,
    pub hibachi_api_secret: String,
    pub lighter_api_key: String,
    pub lighter_api_secret: String,
}

#[allow(unused)]
impl Wallet {
    /// Creates a new Wallet from the given id by loading from "api-keys.json"
    ///
    /// # Arguments
    ///
    /// * `id` - The id of the wallet to load from "api-keys.json".
    ///
    /// # Returns
    ///
    /// # Returns
    ///
    /// * `Wallet` - The wallet struct loaded from "api-keys.json".
    ///
    /// # Errors
    ///
    /// * `anyhow::Error` - If the wallet is not found in "api-keys.json" or if the JSON is invalid.
    pub fn load_from_json(id: u8) -> Result<Self, TradingError> {
        let config = AppConfig::load().map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let file = File::open("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let reader = BufReader::new(file);

        // The JSON in api-keys.json is a map from id (as string) to wallet values
        let wallets_map: serde_json::Value = serde_json::from_reader(reader).map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        // Convert id to string for lookup, eg: "1"
        let id_key = id.to_string();
        let wallet_value = wallets_map.get(&id_key)
            .ok_or_else(|| TradingError::InvalidInput(format!("Wallet id '{}' not found in api-keys.json", id)))?;

        // Deserialize the found value to the Wallet struct
        let mut wallet: Wallet = serde_json::from_value(wallet_value.clone()).map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        wallet.private_key = encode::decrypt_private_key(&wallet.private_key, &config.database.password).unwrap();

        Ok(wallet)
    }
}