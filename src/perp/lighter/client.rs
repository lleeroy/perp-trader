use std::{fs::File, io::BufReader};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::{header::HeaderMap, Method};
use rust_decimal::Decimal;
use urlencoding;
use alloy::signers::{Signature, SignerSync};
use alloy::signers::local::PrivateKeySigner;
use alloy::primitives::eip191_hash_message;
use crate::{error::TradingError, model::{balance::Balance, token::Token, Exchange, Position, PositionSide}, perp::{lighter::{models::{LighterAccount, LighterOrder}, signer::SignerClient}, PerpExchange}, request::Request, trader::wallet::Wallet};
use crate::perp::lighter::signer::TX_TYPE_CHANGE_PUB_KEY;

#[allow(unused)]
pub struct LighterClient {
    wallet: Wallet,
    account_index: u32,
    base_url: String,
    api_private_key: String,
    api_public_key: String,
    signer_client: SignerClient,
}

impl LighterClient {
    /// Creates a new `LighterClient` instance using the provided wallet credentials.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Reference to the user's wallet struct containing API secrets for authentication.
    ///
    /// # Returns
    ///
    /// * `LighterClient` - A client instance ready to communicate with the Lighter API.
    pub async fn new(wallet: &Wallet) -> Result<Self, TradingError> {
        let base_url = "https://mainnet.zklighter.elliot.ai/api/v1".to_string();
        let api_key_index = 0; // Default API key index

        // Get account index
        let account_index = if wallet.lighter_account_index == 0 {
            info!("#{} | Fetching account index from API...", wallet.id);
            let index = Self::get_account_index(&base_url, wallet).await?;
            Self::save_api_key_and_account_index(wallet, &wallet.lighter_api_key, index).await?;
            index
        } else {
            wallet.lighter_account_index
        };

        // Get or generate API key
        let (api_private_key, api_public_key) = if !wallet.lighter_api_key.is_empty() 
            && !wallet.lighter_api_public_key.is_empty() {
            // Use stored API keys
            info!("#{} | Using stored Lighter API keys...", wallet.id);
            (wallet.lighter_api_key.clone(), wallet.lighter_api_public_key.clone())
        } else {
            // Try to fetch existing API key from server
            info!("#{} | No stored API key found. Checking server...", wallet.id);
            
            match Self::get_existing_api_key(&base_url, account_index).await {
                Ok(public_key) => {
                    info!("#{} | Found existing API key on server: {}", wallet.id, public_key);
                    warn!("#{} | Private key not stored locally. Generating NEW API key to replace it...", wallet.id);
                    
                    // Generate and register a new key (this will replace the old one)
                    let (priv_key, pub_key) = Self::register_new_api_key(
                        &base_url, 
                        wallet, 
                        account_index, 
                        api_key_index
                    ).await?;
                    
                    // Save the new keys
                    Self::save_api_keys(wallet, &priv_key, &pub_key, account_index).await?;
                    (priv_key, pub_key)
                }
                Err(_) => {
                    // No existing key, need to generate and register
                    info!("#{} | No API key found on server. Generating new one...", wallet.id);
                    let (priv_key, pub_key) = Self::register_new_api_key(
                        &base_url, 
                        wallet, 
                        account_index, 
                        api_key_index
                    ).await?;
                    
                    // Save the new keys
                    Self::save_api_keys(wallet, &priv_key, &pub_key, account_index).await?;
                    (priv_key, pub_key)
                }
            }
        };

        // Create signer client with the API key
        let signer_client = SignerClient::new(
            &base_url,
            &api_private_key,
            api_key_index,
            account_index as i64,
            None,
            None,
        ).map_err(|e| TradingError::SigningError(e.to_string()))?;

        Ok(Self { 
            wallet: wallet.clone(),
            account_index,
            base_url,
            api_private_key,
            api_public_key,
            signer_client,
        })
    }


    #[allow(unused)]
    pub async fn is_authenticated(&self) -> bool {
        self.get_account().await.is_ok()
    }

    pub async fn get_account(&self) -> Result<LighterAccount, TradingError> {
        let url = format!("{}/account?by=l1_address&value={}", self.base_url, self.wallet.address);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["accounts"].as_array() {
            Some(accounts) => {
                if accounts.is_empty() {
                    return Err(TradingError::InvalidInput("No accounts found".to_string()));
                }

                let account = serde_json::from_value(accounts[0].clone())
                    .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

                Ok(account)
            },
            None => return Err(TradingError::InvalidInput("Accounts not found".to_string())),
        }
    }

    /// Fetches existing API keys from the server and returns the first one's public key
    async fn get_existing_api_key(base_url: &str, account_index: u32) -> Result<String, TradingError> {
        let url = format!("{}/apikeys?account_index={}", base_url, account_index);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["api_keys"].as_array() {
            Some(api_keys) => {
                if api_keys.is_empty() {
                    return Err(TradingError::InvalidInput("No API keys found".to_string()));
                }

                match api_keys[0]["public_key"].as_str() {
                    Some(public_key) => Ok(public_key.to_string()),
                    None => return Err(TradingError::InvalidInput(format!("Public key not found in response: {:?}", response))),
                }
            },
            None => return Err(TradingError::InvalidInput(format!("API keys not found in response: {:?}", response))),
        }
    }
    
    /// Registers a new API key with the Lighter protocol by signing with Ethereum wallet
    async fn register_new_api_key(
        base_url: &str,
        wallet: &Wallet,
        account_index: u32,
        api_key_index: i32,
    ) -> Result<(String, String), TradingError> {
        info!("#{} | Starting API key registration process...", wallet.id);
        
        // Step 1: Generate new API key pair
        let (api_private_key, api_public_key) = SignerClient::create_api_key("")?;
        info!("#{} | Generated new API key pair", wallet.id);
        info!("#{} | Public key: {}", wallet.id, api_public_key);
        
        // Step 2: Get the next nonce for this account
        let nonce_url = format!("{}/nextNonce?account_index={}&api_key_index={}", 
            base_url, account_index, api_key_index);
        let nonce_response = Request::process_request(
            Method::GET, 
            nonce_url, 
            None, 
            None, 
            None
        ).await?;
        
        let nonce = nonce_response["nonce"].as_i64()
            .ok_or_else(|| TradingError::InvalidInput(
                format!("Failed to get nonce: {:?}", nonce_response)
            ))?;
        
        info!("#{} | Got nonce: {}", wallet.id, nonce);
        
        // Step 3: Create a temporary SignerClient with the NEW API key to sign the registration
        // This is the key insight: we use the NEW key to sign its own registration
        let temp_signer = SignerClient::new(
            base_url,
            &api_private_key,
            api_key_index,
            account_index as i64,
            None,
            None,
        )?;
        
        // Step 4: Sign the change API key transaction
        let mut tx_info = temp_signer.sign_change_api_key(&api_public_key, nonce)?;
        
        info!("#{} | Signed change API key transaction", wallet.id);
        
        // Step 5: Extract the message to sign with Ethereum wallet
        let message_to_sign = tx_info.message_to_sign
            .ok_or_else(|| TradingError::SigningError("No MessageToSign in response".to_string()))?;
        
        info!("#{} | Message to sign: {}", wallet.id, message_to_sign);
        
        // Step 6: Sign the message with Ethereum wallet
        let eth_signer = wallet.private_key.parse::<PrivateKeySigner>()
            .map_err(|e| TradingError::SigningError(format!("Invalid private key: {}", e)))?;
        
        // Create the message hash using EIP-191
        let message_hash = eip191_hash_message(message_to_sign.as_bytes());
        
        // Sign the hash
        let signature: Signature = eth_signer.sign_hash_sync(&message_hash)
            .map_err(|e| TradingError::SigningError(e.to_string()))?;
        
        // Convert signature to hex string with 0x prefix
        let signature_hex = signature.to_string();
        
        info!("#{} | Ethereum signature: {}", wallet.id, signature_hex);
        
        // Step 7: Add L1 signature to transaction info
        tx_info.l1_sig = Some(signature_hex);
        tx_info.message_to_sign = None; // Remove MessageToSign from final payload
        
        // Step 8: Send the signed transaction to the API
        let tx_info_json = serde_json::to_string(&tx_info)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        
        info!("#{} | Sending registration transaction to API...", wallet.id);
        
        let tx_info_encoded = urlencoding::encode(&tx_info_json);
        let body = format!("tx_type={}&tx_info={}", TX_TYPE_CHANGE_PUB_KEY, tx_info_encoded);
        
        // Create headers for the request
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::header::HeaderValue::from_static("application/x-www-form-urlencoded;charset=UTF-8"),
        );
        
        let send_tx_url = format!("{}/sendTx", base_url);
        let response = Request::process_request(
            Method::POST,
            send_tx_url,
            Some(headers),
            Some(body),
            None,
        ).await?;
        
        // Step 9: Check response
        if let Some(code) = response["code"].as_i64() {
            if code != 200 {
                let message = response["message"].as_str()
                    .unwrap_or("Unknown error");
                return Err(TradingError::InvalidInput(
                    format!("API key registration failed: {} (code: {})", message, code)
                ));
            }
        }
        
        info!("#{} | API key registered successfully!", wallet.id);
        info!("#{} | Response: {:?}", wallet.id, response);
        
        Ok((api_private_key, api_public_key))
    }

    async fn get_account_index(base_url: &str, wallet: &Wallet) -> Result<u32, TradingError> {
        let url = format!("{}/accountsByL1Address?l1_address={}", base_url, wallet.address);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["sub_accounts"][0]["index"].as_u64() {
            Some(index) => Ok(index as u32),
            None => return Err(TradingError::InvalidInput(format!("Account index not found in response: {:?}", response))),
        }
    }


    async fn get_nonce(&self) -> Result<i64, TradingError> {
        let url = format!("{}/nextNonce?account_index={}&api_key_index=0", self.base_url, self.account_index);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

        match response["nonce"].as_i64() {
            Some(nonce) => Ok(nonce),
            None => return Err(TradingError::InvalidInput(format!("Nonce not found in response: {:?}", response))),
        }
    }

    async fn get_market_price(&self, token: &Token) -> Result<u64, TradingError> {
        let end_timestamp = Utc::now().timestamp_millis();
        let start_timestamp = end_timestamp - 60000;
        let url = format!("{}/candlesticks?market_id={}&resolution=1m&start_timestamp={}&end_timestamp={}&count_back=0", self.base_url, token.get_market_index(Exchange::Lighter), start_timestamp, end_timestamp);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;
        
        match response["candlesticks"].as_array() {
            Some(candlesticks) => {

                if candlesticks.is_empty() {
                    return Err(TradingError::MarketDataUnavailable(
                        format!("No candlesticks found for token {} on Lighter", 
                        token.get_symbol_string(Exchange::Lighter))
                    ));
                }

                let price_f64 = candlesticks[0]["close"].as_f64().unwrap();
                let price = (price_f64 * 100.0).round() as u64;
                Ok(price)
            },
            None => return Err(TradingError::InvalidInput(format!("Candlesticks not found in response: {:?}", response))),
        }
    }

    async fn calculate_base_amount(&self, balance_usdc: Decimal, price: u64) -> Result<u64, TradingError> {
        // Price is stored as integer with 2 decimal places (e.g., 387424 = 3874.24$)
        // Convert price to proper decimal format by dividing by 100
        let price_decimal = Decimal::from(price) / Decimal::from(100);
        
        // Calculate base amount: balance_usdc / price
        // This gives us the amount of base token we can buy
        let base_amount = balance_usdc / price_decimal;
        
        // Convert to integer (base token amount is typically stored as integer)
        // For example: 10.0 USDC / 3874.24 = 0.00258... -> 0.00258 * 10000 = 25
        let base_amount_scaled = base_amount * Decimal::from(10000);
        
        Ok(base_amount_scaled.round().to_string().parse::<u64>().unwrap())
    }

    async fn execute_market_buy_order(
        &self,
        token: Token,
        side: PositionSide,
        base_amount: u64,
        price: u64,
    ) -> Result<String, TradingError> {
        let nonce = self.get_nonce().await?;
        let market_index = token.get_market_index(Exchange::Lighter);

        let is_ask = match side {
            PositionSide::Long => false,
            PositionSide::Short => true,
        };

        info!("#{} | Executing market {} order for {:?} with price {} | nonce: {}", self.wallet.id, side, token, price, nonce);

        let order = LighterOrder::new(
            self.account_index,
            market_index,
            base_amount as i64,
            price as i64,
            is_ask,
            nonce,
        );

        let order_signed = self.signer_client.sign_create_order(
            market_index,
            order.tx_info.client_order_index,
            order.tx_info.base_amount,
            order.tx_info.price,
            order.tx_info.is_ask,
            order.tx_info.type_field,
            order.tx_info.time_in_force,
            order.tx_info.reduce_only,
            order.tx_info.trigger_price,
            order.tx_info.order_expiry,
            order.tx_info.nonce,
        )?;

        let order_id = self.send_order(order.tx_type, &order_signed, order.price_protection).await?;
        Ok(order_id)
    }

    
    async fn send_order(&self, tx_type: i32, order_signed: &str, price_protection: bool) -> Result<String, TradingError> {
        let headers = self.get_headers();
        let url = format!("{}/sendTx", self.base_url);
        let tx_info_encoded = urlencoding::encode(order_signed);
        
        let body = format!(
            "tx_type={}&tx_info={}&price_protection={}",
            tx_type,
            tx_info_encoded,
            price_protection
        );

        let response = Request::process_request(
            Method::POST, 
            url, 
            Some(headers), 
            Some(body), 
            None
        ).await?;
        
        println!("Response: {:#?}", response);
        // Return the response as string or extract order ID
        Ok(serde_json::to_string(&response).unwrap_or_else(|_| "Order sent successfully".to_string()))
    }

    /// Saves both API keys (private and public) and account index to api-keys.json
    async fn save_api_keys(
        wallet: &Wallet, 
        api_private_key: &str, 
        api_public_key: &str,
        account_index: u32
    ) -> Result<(), TradingError> {
        // Strip "0x" prefix if present to keep storage format consistent (40 chars without prefix)
        let clean_private_key = api_private_key.trim_start_matches("0x");
        let clean_public_key = api_public_key.trim_start_matches("0x");
        
        let file = File::open("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut wallets_map: serde_json::Value = serde_json::from_reader(reader)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let id_key = wallet.id.to_string();

        if let serde_json::Value::Object(ref mut map) = wallets_map {
            if let Some(wallet_value) = map.get_mut(&id_key) {
                if let serde_json::Value::Object(ref mut wallet_obj) = wallet_value {
                    wallet_obj.insert(
                        "lighter_api_key".to_string(),
                        serde_json::Value::String(clean_private_key.to_string()),
                    );
                    wallet_obj.insert(
                        "lighter_api_public_key".to_string(),
                        serde_json::Value::String(clean_public_key.to_string()),
                    );
                    wallet_obj.insert(
                        "lighter_account_index".to_string(),
                        serde_json::Value::Number(account_index.into()),
                    );
                } else {
                    return Err(TradingError::InvalidInput(format!(
                        "Wallet entry for id '{}' is not an object",
                        id_key
                    )));
                }
            } else {
                return Err(TradingError::InvalidInput(format!(
                    "Wallet with id '{}' not found in api-keys.json",
                    id_key
                )));
            }
        } else {
            return Err(TradingError::InvalidInput(
                "api-keys.json does not contain a valid object".to_string(),
            ));
        }

        let file = File::create("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        serde_json::to_writer_pretty(file, &wallets_map)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        info!("#{} | Lighter API keys saved successfully...", wallet.id);
        Ok(())
    }

    /// Updates the lighter_api_key field for the wallet with the given id in api-keys.json.
    async fn save_api_key_and_account_index(wallet: &Wallet, api_key: &str, account_index: u32) -> Result<(), TradingError> {
        let file = File::open("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut wallets_map: serde_json::Value = serde_json::from_reader(reader)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        let id_key = wallet.id.to_string();

        if let serde_json::Value::Object(ref mut map) = wallets_map {
            if let Some(wallet_value) = map.get_mut(&id_key) {
                if let serde_json::Value::Object(ref mut wallet_obj) = wallet_value {
                    if !api_key.is_empty() {
                        wallet_obj.insert(
                            "lighter_api_key".to_string(),
                            serde_json::Value::String(api_key.to_string()),
                        );
                    }
                    wallet_obj.insert(
                        "lighter_account_index".to_string(),
                        serde_json::Value::Number(account_index.into()),
                    );
                } else {
                    return Err(TradingError::InvalidInput(format!(
                        "Wallet entry for id '{}' is not an object",
                        id_key
                    )));
                }
            } else {
                return Err(TradingError::InvalidInput(format!(
                    "Wallet with id '{}' not found in api-keys.json",
                    id_key
                )));
            }
        } else {
            return Err(TradingError::InvalidInput(
                "api-keys.json does not contain a valid object".to_string(),
            ));
        }

        let file = File::create("api-keys.json").map_err(|e| TradingError::InvalidInput(e.to_string()))?;
        serde_json::to_writer_pretty(file, &wallets_map)
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        info!("#{} | Lighter account index saved successfully...", wallet.id);
        Ok(())
    }

    fn get_headers(&self) -> HeaderMap {
        use http::header::{HeaderMap, HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();

        headers.insert(HeaderName::from_static("connection"), HeaderValue::from_static("keep-alive"));
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/x-www-form-urlencoded;charset=UTF-8"),
        );
        headers.insert(
            HeaderName::from_static("origin"),
            HeaderValue::from_static("https://app.lighter.xyz"),
        );
        headers.insert(
            HeaderName::from_static("preferauthserver"),
            HeaderValue::from_static("true"),
        );
        headers.insert(
            HeaderName::from_static("referer"),
            HeaderValue::from_static("https://app.lighter.xyz/"),
        );
        headers.insert(
            HeaderName::from_static("sec-fetch-dest"),
            HeaderValue::from_static("empty"),
        );
        headers.insert(
            HeaderName::from_static("sec-fetch-mode"),
            HeaderValue::from_static("cors"),
        );
        headers.insert(
            HeaderName::from_static("sec-fetch-site"),
            HeaderValue::from_static("cross-site"),
        );
        headers.insert(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
            ),
        );
        headers.insert(
            HeaderName::from_static("sec-ch-ua"),
            HeaderValue::from_static(r#"\"Google Chrome\";v=\"141\", \"Not?A_Brand\";v=\"8\", \"Chromium\";v=\"141\""#),
        );
        headers.insert(
            HeaderName::from_static("sec-ch-ua-mobile"),
            HeaderValue::from_static("?0"),
        );
        headers.insert(
            HeaderName::from_static("sec-ch-ua-platform"),
            HeaderValue::from_static(r#""macOS""#),
        );

        headers
    }
}

#[async_trait]
impl PerpExchange for LighterClient {
    fn name(&self) -> &str {
        "Lighter"
    }

    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }

    async fn get_balance(&self, _asset: &str) -> Result<Balance, TradingError> {
        let available_balance = self.get_account().await?
            .available_balance
            .parse::<Decimal>()
            .map_err(|e| TradingError::InvalidInput(e.to_string()))?;

        Ok(Balance {
            asset: "USDC".to_string(),
            free: available_balance,
            locked: Decimal::ZERO,
        })
    }

    async fn get_balances(&self) -> Result<Vec<Balance>, TradingError> {
        // TODO: Implement actual API call
        log::warn!("Lighter get_balances not fully implemented");
        Ok(vec![])
    }

    async fn open_position(&self, token: Token, side: PositionSide, _amount_usdc: Decimal) -> Result<Position, TradingError> {
        let balance_usdc = self.get_usdc_balance().await?;
        let price = self.get_market_price(&token).await?;
        let base_amount = self.calculate_base_amount(balance_usdc, price).await?;

        if balance_usdc < Decimal::from(10) {
            return Err(TradingError::InsufficientBalance("Insufficient balance for USDC".to_string()));
        }

        let order = self.execute_market_buy_order(token, side, base_amount, price).await?;
        info!("#{} | Order sent: {}", self.wallet.id, order);
        loop {};

        todo!("Implement position opening");
    }


    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        let balance_usdc = self.get_balance("USDC").await?;
        Ok(balance_usdc.free)
    }

    async fn close_position(&self, position: &Position) -> Result<Position, TradingError> {
        // TODO: Implement actual API call
        todo!("Lighter close_position not fully implemented for {}", position.side);
    }
}

