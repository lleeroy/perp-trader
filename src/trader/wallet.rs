use std::{fs::File, io::BufReader};
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use solana_sdk::signature::{Keypair, Signer};
use crate::{config::AppConfig, error::TradingError, helpers::encode, perp::lighter::client::LighterClient};

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
    pub lighter_api_key: String,
    pub solana_private_key: Option<String>,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct WalletTradingClient {
    pub wallet: Wallet,
    pub lighter_client: LighterClient,
}

impl WalletTradingClient {
    pub async fn new(wallet: Wallet) -> Result<Self, TradingError> {
        let lighter_client = LighterClient::new(&wallet).await?;

        Ok(WalletTradingClient { wallet, lighter_client })
    }
}

#[allow(unused)]
impl Wallet {
    /// Creates a new Wallet from the given id by loading from "api-keys.json"
    pub fn load_from_json(id: u8) -> Result<Self, TradingError> {
        use serde_json::Value;

        let config = AppConfig::load()
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let password = std::env::var("WALLETS_PASSWORD")
            .context("Failed to get WALLETS_PASSWORD from environment variables")?;

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

        let lighter_api_key = wallet_value.get("lighter_api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field lighter_api_key".into()))?
            .to_string();

        let backpack_api_key = wallet_value.get("backpack_api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field backpack_api_key".into()))?
            .to_string();

        let backpack_api_secret = wallet_value.get("backpack_api_secret")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field backpack_api_secret".into()))?
            .to_string();

        let decrypted_private_key = encode::decrypt_private_key(&private_key, &password)
            .map_err(|e| TradingError::InvalidInput(format!("Failed to decrypt private key: {e}")))?;

        if address.is_empty() {
            return Err(TradingError::InvalidInput("Address is empty".to_string()));
        }
        if decrypted_private_key.is_empty() {
            return Err(TradingError::InvalidInput("Private key is empty".to_string()));
        }

        let solana_private_key = wallet_value.get("solana_private_key")
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            })
            .unwrap_or(None);

        Ok(Wallet {
            id,
            proxy,
            private_key: decrypted_private_key,
            address,
            backpack_api_key,
            backpack_api_secret,
            lighter_api_key,
            solana_private_key,
        })
    }


    /// Sign a message with the Solana private key
    /// 
    /// # Arguments
    /// * `message` - The message bytes to sign
    /// 
    /// # Returns
    /// * `Result<Vec<u8>, TradingError>` - The signature bytes
    pub fn sign_solana_message(&self, message: &[u8]) -> Result<Vec<u8>, TradingError> {
        let private_key = self.solana_private_key
            .as_ref()
            .ok_or_else(|| TradingError::InvalidInput("Solana private key not found".to_string()))?;

        // Create keypair from bytes
        let keypair = Keypair::from_base58_string(&private_key);

        // Sign the message
        let signature = keypair.sign_message(message);

        Ok(signature.as_ref().to_vec())
    }

    /// Create a Solana keypair from the stored private key
    /// This is a helper method for operations that need the keypair directly
    pub fn get_solana_keypair(&self) -> Result<Keypair, TradingError> {
        let private_key = self.solana_private_key
            .as_ref()
            .ok_or_else(|| TradingError::InvalidInput("Solana private key not found".to_string()))?;

        Ok(Keypair::from_base58_string(&private_key))
    }
}