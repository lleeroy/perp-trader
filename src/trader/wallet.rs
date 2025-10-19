use std::{fs::File, io::BufReader};
use serde::Deserialize;
use anyhow::{Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Wallet {
    pub id: u8,
    pub backpack_api_key: String,
    pub backpack_api_secret: String,
    pub hibachi_api_key: String,
    pub hibachi_api_secret: String,
}

impl Wallet {
    /// Creates a new Wallet from the given id by loading from "api-keys.json"
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