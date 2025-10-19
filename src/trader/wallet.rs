use std::{fs::File, io::BufReader};
use serde::Deserialize;
use anyhow::{Result};

/// Wallet struct containing API secrets for authentication with exchanges
#[derive(Debug, Clone, Deserialize)]
#[allow(unused)]
pub struct Wallet {
    pub id: u8,
    pub backpack_api_key: String,
    pub backpack_api_secret: String,
    pub hibachi_api_key: String,
    pub hibachi_api_secret: String,
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
    pub fn load_from_json(id: u8) -> Result<Self> {
        let file = File::open("api-keys.json")?;
        let reader = BufReader::new(file);

        // The JSON in api-keys.json is a map from id (as string) to wallet values
        let wallets_map: serde_json::Value = serde_json::from_reader(reader)?;

        // Convert id to string for lookup, eg: "1"
        let id_key = id.to_string();
        let wallet_value = wallets_map.get(&id_key)
            .ok_or_else(|| anyhow::anyhow!("Wallet id '{}' not found in api-keys.json", id))?;

        // Deserialize the found value to the Wallet struct
        let wallet: Wallet = serde_json::from_value(wallet_value.clone())?;
        Ok(wallet)
    }
}