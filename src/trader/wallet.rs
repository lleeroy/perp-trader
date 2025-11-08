use std::{fs::File, io::BufReader};
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use solana_sdk::signature::{Keypair, Signer};
use crate::{config::AppConfig, error::TradingError, helpers::encode, perp::lighter::client::LighterClient, storage::database::Database};

/// Wallet struct containing API secrets for authentication with exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(unused)]
pub struct Wallet {
    pub id: u8,
    pub private_key: String,
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
    /// Proxy, if present, is loaded from "proxies.json" using wallet id as key.
    ///
    /// If the wallet with given id is not present in api-keys.json,
    /// returns an error that it's not enabled. If present but missing
    /// a private key, will try get the private key from DB.
    pub async fn load_from_json(id: u8) -> Result<Self, TradingError> {
        use serde_json::Value;

        // config is required, but even if want DB, error message should match previous behavior
        let _config = AppConfig::load()
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let password = std::env::var("WALLETS_PASSWORD")
            .context("Failed to get WALLETS_PASSWORD from environment variables")?;

        // Load API keys from file
        let file = File::open("api-keys.json")
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let reader = BufReader::new(file);

        let wallets_map: Value = serde_json::from_reader(reader)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let id_key = id.to_string();
        let wallet_value = wallets_map.get(&id_key)
            .ok_or_else(|| TradingError::InvalidInput(format!(
                "Wallet id '{}' not enabled (not found in api-keys.json)",
                id,
            )))?;

        // Extract lighter_api_key from JSON
        let lighter_api_key = wallet_value.get("lighter_api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TradingError::InvalidInput("Missing field lighter_api_key".into()))?
            .to_string();

        // solana_private_key may be missing
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

        // Try get wallet private key from file
        let file_private_key = wallet_value.get("private_key")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty()); // treat empty string as missing

        let decrypted_private_key = if let Some(encrypted_key) = file_private_key {
            encode::decrypt_private_key(encrypted_key, &password)
                .map_err(|e| TradingError::InvalidInput(format!("Failed to decrypt private key: {e}")))?
        } else {
            // If no private_key in file, get it from the DB
            let database = Database::get_instance().await?;
            let account = database.get_account_by_id(id as u32).await;

            match account {
                Ok(Some(account)) => account.wallet_key,
                Ok(None) => return Err(TradingError::InvalidInput(format!(
                    "Private key is missing for wallet id '{}' (both in api-keys.json and db)",
                    id
                ))),
                Err(e) => return Err(TradingError::InvalidInput(format!(
                    "Error getting private key from db: {e}"
                ))),
            }
        };

        if decrypted_private_key.is_empty() {
            return Err(TradingError::InvalidInput("Private key is empty".to_string()));
        }

        // Load proxy from "proxies.json"
        let proxy = {
            let proxy_file = File::open("proxies.json")
                .map_err(|e| TradingError::InvalidInput(format!("Error opening proxies.json: {}", e)))?;
            let proxy_reader = BufReader::new(proxy_file);
            let proxies_map: Value = serde_json::from_reader(proxy_reader)
                .map_err(|e| TradingError::InvalidInput(format!("Error reading proxies.json: {}", e)))?;

            // If id entry not present, return None. If present but proxy: "", then None. Else, Some(proxy string)
            match proxies_map.get(&id_key) {
                Some(wallet_proxy_value) => wallet_proxy_value.get("proxy")
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    })
                    .unwrap_or(None),
                None => None,
            }
        };

        Ok(Wallet {
            id,
            proxy,
            private_key: decrypted_private_key,
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

    /// Retrieves the Ethereum wallet address from the stored private key using alloy,
    /// returning the address in EIP-55 checksum (original on-chain) format.
    pub fn get_ethereum_address(&self) -> Result<String, TradingError> {
        use alloy::signers::local::PrivateKeySigner;
        use alloy::primitives::Address;

        let wallet = self.private_key
            .parse::<PrivateKeySigner>()
            .map_err(|e| TradingError::InvalidInput(format!("Invalid Ethereum private key: {e}")))?;

        let address: Address = wallet.address();
        Ok(address.to_checksum(None))
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