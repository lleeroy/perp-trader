use std::{fs::File, io::BufReader};
use serde::{Deserialize, Serialize};
use anyhow::{Result};

use crate::{config::AppConfig, error::TradingError, helpers::encode, perp::{backpack::BackpackClient, lighter::client::LighterClient}};

/// Wallet struct containing API secrets for authentication with exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(unused)]
pub struct Wallet {
    pub id: u8,
    pub private_key: String,
    pub address: String,
    pub backpack_api_key: String,
    pub backpack_api_secret: String,
    pub proxy: Option<String>,
}


#[allow(unused)]
#[derive(Debug, Clone)]
pub struct WalletTradingClient {
    pub wallet: Wallet,
    pub lighter_client: LighterClient,
    pub backpack_client: BackpackClient,
}

impl WalletTradingClient {
    pub async fn new(wallet: Wallet) -> Result<Self, TradingError> {
        let lighter_client = LighterClient::new(&wallet).await?;
        let backpack_client = BackpackClient::new(&wallet);

        Ok(WalletTradingClient { wallet, lighter_client, backpack_client })
    }
}

#[allow(unused)]
impl Wallet {
    /// Creates a new Wallet from the given id by loading from "api-keys.json"
    ///
    /// This only parses the relevant fields from JSON, then creates a LighterClient with those fields and unites everything in the Wallet struct.
    pub fn load_from_json(id: u8) -> Result<Self, TradingError> {
        use serde_json::Value;

        let config = AppConfig::load()
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let file = File::open("api-keys.json")
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let reader = BufReader::new(file);

        let wallets_map: Value = serde_json::from_reader(reader)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let id_key = id.to_string();
        let wallet_value = wallets_map.get(&id_key)
            .ok_or_else(|| TradingError::InvalidInput(format!("Wallet id '{}' not found in api-keys.json", id)))?;

        // Extract relevant fields manually from the json value
        let private_key = wallet_value.get("private_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field private_key".into()))?
            .to_string();

        let address = wallet_value.get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field address".into()))?
            .to_string();

        let proxy = wallet_value.get("proxy")
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            })
            .unwrap_or(None);

        let backpack_api_key = wallet_value.get("backpack_api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field backpack_api_key".into()))?
            .to_string();

        let backpack_api_secret = wallet_value.get("backpack_api_secret")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field backpack_api_secret".into()))?
            .to_string();

        let decrypted_private_key = encode::decrypt_private_key(&private_key, &config.database.password)
            .map_err(|e| TradingError::InvalidInput(format!("Failed to decrypt private key: {e}")))?;

        if address.is_empty() {
            return Err(TradingError::InvalidInput("Address is empty".to_string()));
        }
        if decrypted_private_key.is_empty() {
            return Err(TradingError::InvalidInput("Private key is empty".to_string()));
        }

        Ok(Wallet {
            id,
            proxy,
            private_key: decrypted_private_key,
            address,
            backpack_api_key,
            backpack_api_secret,
        })
    }
}