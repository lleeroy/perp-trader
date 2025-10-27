use std::str::FromStr;
use std::time::Duration;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{header::HeaderMap, Method};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use tokio::time::sleep;
use urlencoding;
use alloy::signers::{Signature, SignerSync};
use alloy::signers::local::PrivateKeySigner;
use alloy::primitives::eip191_hash_message;
use crate::error::RequestError;
use crate::model::PositionStatus;
use crate::perp::lighter::models::{LighterPosition, LighterTx};
use crate::{error::TradingError, model::{balance::Balance, token::Token, Exchange, Position, PositionSide}, perp::{lighter::{models::{LighterAccount, LighterOrder}, signer::SignerClient}, PerpExchange}, request::Request, trader::wallet::Wallet};
use crate::perp::lighter::signer::{TX_TYPE_CHANGE_PUB_KEY, TX_TYPE_CREATE_ORDER, TX_TYPE_UPDATE_LEVERAGE};

const DEFAULT_API_KEY_INDEX: i32 = 0;
const DEFAULT_BASE_URL: &str = "https://mainnet.zklighter.elliot.ai/api/v1";


/// A client for interacting with the Lighter perpetual futures exchange.
/// 
/// The `LighterClient` provides a high-level interface for trading perpetual futures
/// on the Lighter exchange, including position management, order execution, and
/// account operations. It handles authentication, signing, and API communication
/// through a combination of Ethereum wallet signatures and API key authentication.
/// 
/// # Features
/// 
/// * Automated API key registration and authentication
/// * Market order execution with retry logic
/// * Position management (open/close)
/// * Real-time market data and price feeds
/// * Account balance and portfolio tracking
/// * Nonce management for transaction sequencing
/// 
/// # Authentication
/// 
/// The client uses a two-layer authentication system:
/// 1. Ethereum wallet signature for initial API key registration
/// 2. Generated API key pair for subsequent API calls
#[allow(unused)]
#[derive(Debug, Clone)]
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
    /// This method performs the complete initialization process:
    /// 1. Retrieves the account index from the Lighter API
    /// 2. Generates and registers a new API key pair
    /// 3. Initializes the signing client for transaction authorization
    ///
    /// # Arguments
    ///
    /// * `wallet` - Reference to the user's wallet containing API secrets and Ethereum private key
    ///
    /// # Returns
    ///
    /// * `Result<Self, TradingError>` - A fully initialized LighterClient ready for trading operations
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Account index cannot be retrieved
    /// * API key registration fails
    /// * Signer client initialization fails
    pub async fn new(wallet: &Wallet) -> Result<Self, TradingError> {
        let base_url = DEFAULT_BASE_URL.to_string();     // Default base URL
        let api_key_index = DEFAULT_API_KEY_INDEX;          // Default API key index
        let account_index = Self::get_account_index(&base_url, wallet).await?;

        let (api_private_key, api_public_key) = Self::register_new_api_key(
            &base_url, 
            wallet, 
            account_index, 
            api_key_index
        ).await?;

        
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


    /// Checks if the client is properly authenticated with the Lighter API.
    ///
    /// This method verifies that the API credentials are valid and the client
    /// can successfully access account information.
    ///
    /// # Returns
    ///
    /// * `bool` - `true` if authentication is successful, `false` otherwise
    #[allow(unused)]
    pub async fn is_authenticated(&self) -> bool {
        self.get_account().await.is_ok()
    }

    
    /// Registers a new API key with the Lighter protocol using Ethereum wallet signature.
    ///
    /// This method handles the complete API key registration flow:
    /// 1. Generates a new ECDSA key pair for API authentication
    /// 2. Retrieves the current nonce for the account
    /// 3. Signs the API key registration transaction with the Ethereum wallet
    /// 4. Submits the signed transaction to the Lighter API
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base URL of the Lighter API
    /// * `wallet` - Reference to the wallet containing Ethereum credentials
    /// * `account_index` - The account index to register the API key for
    /// * `api_key_index` - The index for the new API key
    ///
    /// # Returns
    ///
    /// * `Result<(String, String), TradingError>` - Tuple containing (private_key, public_key)
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Nonce retrieval fails
    /// * Message signing fails
    /// * Transaction submission fails
    /// * API returns non-200 response
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
            wallet.proxy.clone()
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
            wallet.proxy.clone(),
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
        Ok((api_private_key, api_public_key))
    }


    /// Retrieves the complete account information from the Lighter API.
    ///
    /// This method fetches the account details including balances, positions,
    /// and other account-specific data.
    ///
    /// # Returns
    ///
    /// * `Result<LighterAccount, TradingError>` - The account information
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * API request fails
    /// * No accounts are found for the wallet
    /// * Response parsing fails
    async fn get_account(&self) -> Result<LighterAccount, TradingError> {
        let url = format!("{}/account?by=l1_address&value={}", self.base_url, self.wallet.address);
        let response = Request::process_request(Method::GET, url, None, None, self.wallet.proxy.clone()).await?;

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


    /// Retrieves the account index for the given wallet from the Lighter API.
    ///
    /// The account index is a unique identifier used in all subsequent API calls
    /// to reference the specific trading account.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base URL of the Lighter API
    /// * `wallet` - Reference to the wallet to get the account index for
    ///
    /// # Returns
    ///
    /// * `Result<u32, TradingError>` - The account index
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * API request fails
    /// * Account index not found in response
    /// * Response parsing fails
    async fn get_account_index(base_url: &str, wallet: &Wallet) -> Result<u32, TradingError> {
        let url = format!("{}/accountsByL1Address?l1_address={}", base_url, wallet.address);
        let response = Request::process_request(Method::GET, url, None, None, wallet.proxy.clone()).await?;

        match response["sub_accounts"][0]["index"].as_u64() {
            Some(index) => Ok(index as u32),
            None => return Err(TradingError::InvalidInput(format!("Account index not found in response: {:?}", response))),
        }
    }

    /// Retrieves the next nonce for transaction sequencing.
    ///
    /// Nonces are used to ensure transaction ordering and prevent replay attacks.
    /// Each transaction must have a unique, sequential nonce.
    ///
    /// # Returns
    ///
    /// * `Result<i64, TradingError>` - The next nonce to use
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * API request fails
    /// * Nonce not found in response
    async fn get_nonce(&self) -> Result<i64, TradingError> {
        let url = format!("{}/nextNonce?account_index={}&api_key_index=0", self.base_url, self.account_index);
        let response = Request::process_request(Method::GET, url, None, None, self.wallet.proxy.clone()).await?;

        match response["nonce"].as_i64() {
            Some(nonce) => Ok(nonce),
            None => return Err(TradingError::InvalidInput(format!("Nonce not found in response: {:?}", response))),
        }
    }

    /// Retrieves the current market price for a token with slippage adjustment.
    ///
    /// This method fetches recent candlestick data and calculates an adjusted
    /// price with built-in slippage protection for order execution.
    ///
    /// # Arguments
    ///
    /// * `token` - Reference to the token to get price for
    /// * `side` - The position side (Long/Short) to adjust slippage accordingly
    ///
    /// # Returns
    ///
    /// * `Result<u64, TradingError>` - The adjusted market price scaled by token denomination
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * API request fails
    /// * No candlestick data available
    /// * Price calculation fails
    ///
    /// # Slippage Adjustment
    ///
    /// * Long positions: +0.5% slippage protection
    /// * Short positions: -0.5% slippage protection
    pub async fn get_market_price(&self, token: &Token, side: PositionSide) -> Result<u64, TradingError> {
        let end_timestamp = Utc::now().timestamp_millis();
        let start_timestamp = end_timestamp - 60000;
        let url = format!("{}/candlesticks?market_id={}&resolution=1m&start_timestamp={}&end_timestamp={}&count_back=5", self.base_url, token.get_market_index(Exchange::Lighter), start_timestamp, end_timestamp);
        let response = Request::process_request(Method::GET, url, None, None, self.wallet.proxy.clone()).await?;


        match response["candlesticks"].as_array() {
            Some(candlesticks) => {

                if candlesticks.is_empty() {
                    return Err(TradingError::MarketDataUnavailable(
                        format!("No candlesticks found for token {} on Lighter", 
                        token.get_symbol_string(Exchange::Lighter))
                    ));
                }

                let latest_candlestick = &candlesticks[candlesticks.len() - 1];
                let price_f64 = latest_candlestick["close"].as_f64().unwrap();
                let price_decimal = Decimal::from_f64(price_f64).unwrap();

                let adjusted_price = match side {
                    PositionSide::Short => price_decimal * Decimal::from_f64(0.995).unwrap(),
                    PositionSide::Long => price_decimal * Decimal::from_f64(1.005).unwrap(),
                };

                // Use token's price denomination to scale the price correctly
                let price_denomination = Decimal::from_f64(token.get_price_denomination()).unwrap();
                let scaled_price = adjusted_price * price_denomination;
                
                // Round to ensure we get a clean integer
                let price = scaled_price.round().to_string().parse::<u64>().unwrap();

                Ok(price)
            },
            None => return Err(TradingError::InvalidInput(format!("Candlesticks not found in response: {:?}", response))),
        }
    }


    /// Retrieves transaction details by hash with retry logic.
    ///
    /// This method attempts to fetch transaction details multiple times to handle
    /// potential API latency or temporary failures.
    ///
    /// # Arguments
    ///
    /// * `hash` - The transaction hash to look up
    ///
    /// # Returns
    ///
    /// * `Result<LighterTx, TradingError>` - The transaction details
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * All retry attempts fail
    /// * Transaction not found
    /// * Response parsing fails
    async fn get_order_by_hash(&self, hash: &str) -> Result<LighterTx, TradingError> {
        let url = format!("{}/tx?by=hash&value={}", self.base_url, hash);
        let mut last_err = None;

        for attempt in 1..=3 {
            let response = match Request::process_request(Method::GET, url.clone(), None, None, self.wallet.proxy.clone()).await {
                Ok(res) => res,
                Err(e) => {
                    last_err = Some(TradingError::OrderExecutionFailed(format!("Attempt {attempt}: request error: {e}")));
                    sleep(Duration::from_millis(350)).await;
                    continue;
                }
            };

            let tx: Result<LighterTx, _> = serde_json::from_value(response.clone());

            match tx {
                Ok(tx) => {
                    if tx.code == 200 {
                        return Ok(tx);
                    } else {
                        last_err = Some(
                            TradingError::OrderExecutionFailed(
                                format!("Attempt {attempt}: Failed to get order by hash.")
                            )
                        );
                    }
                }
                Err(e) => {
                    last_err = Some(TradingError::OrderExecutionFailed(format!("Attempt {attempt}: deserialize error: {e}")));
                }
            }

            // Wait a bit before next attempt if not last try
            if attempt < 3 {
                sleep(Duration::from_millis(350)).await;
            }
        }

        Err(last_err.unwrap_or_else(|| TradingError::OrderExecutionFailed("get_order_by_hash failed after 3 attempts".to_string())))
    }


    /// Retrieves all active (non-zero) positions for the account.
    ///
    /// This method filters out positions with zero value and returns only
    /// positions that have actual exposure.
    ///
    /// # Returns
    ///
    /// * `Result<Vec<LighterPosition>, TradingError>` - List of active positions
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Account retrieval fails
    /// * No positions found in account
    async fn get_active_positions(&self) -> Result<Vec<LighterPosition>, TradingError> {
        let account = self.get_account().await?;
        match account.positions {
            Some(positions) => {
                let positions = positions.iter()
                    .filter(
                        |p| 
                        p.position_value.parse::<Decimal>()
                            .unwrap_or(Decimal::ZERO) > Decimal::ZERO
                        )
                    .cloned()
                    .collect();

                return Ok(positions);
            },
            None => return Err(TradingError::PositionNotFound(format!("No positions found for account #{}", self.account_index))),
        }
    }


    /// Closes all active positions with market orders.
    ///
    /// This method identifies all open positions and executes market orders
    /// to close them completely. It includes verification to ensure positions
    /// are successfully closed.
    ///
    /// # Returns
    ///
    /// * `Result<(), TradingError>` - Success if all positions are closed
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Position retrieval fails
    /// * Order execution fails for any position
    /// * Position verification fails
    async fn close_all_positions(&self) -> Result<(), TradingError> {
        let positions = self.get_active_positions().await?;
        let positions_to_close: Vec<LighterPosition> = positions.iter()
            .filter(|p| self.should_close_position(p))
            .cloned()
            .collect();
    
        if positions_to_close.is_empty() {
            info!("#{} | no positions to close!", self.wallet.id);
            return Ok(());
        }
    
        for position in positions_to_close {
            let token_id = position.market_id;
            let token = Token::from_market_index(Exchange::Lighter, token_id);
            let (position_side_current, position_side_to_close) = self.parse_position_sides(position.sign)?;
    
            let price = self.get_market_price(&token, position_side_to_close).await?;
    
            info!("#{} | found open {} position to close: {}", self.wallet.id, position_side_current, position.symbol);
            let position_size: f64 = position.position.parse::<f64>().unwrap();
            let base_amount = self.base_amount_from_f64(position_size)?;
    
            info!("#{} | <{}> position size: {}", self.wallet.id, position.symbol, position_size);
            info!("#{} | <{}> base amount: {}", self.wallet.id, position.symbol, base_amount);
    
            let order = self
                .execute_market_order(&token, position_side_to_close, base_amount, price, true).await?;
    
            match self.get_order_by_hash(&order).await {
                Ok(_) => {
                    info!("#{} | found order by hash: {}", self.wallet.id, order);
                    let positions = self.get_active_positions().await?;
                    let market_index = token.get_market_index(Exchange::Lighter);
    
                    info!("#{} | looking in positions if still open...", self.wallet.id);
    
                    if let Some(pos) = positions.iter().find(|p| p.market_id == market_index) {
                        let pos_value = pos.position_value.parse::<Decimal>().unwrap_or(Decimal::ZERO);
                        info!("#{} | position value: {}", self.wallet.id, pos_value);
    
                        if pos_value == Decimal::ZERO {
                            info!("#{} | position size is 0, which means it closed: {}", self.wallet.id, pos.symbol);
                            info!("#{} | 🔴🔴 position closed: {}", self.wallet.id, pos.symbol);

                            return Ok(());
                        } else {
                            return Err(TradingError::ExchangeError(format!(
                                "#{} | failed to close position on market index {} with token {}, it's still open...",
                                self.wallet.id, market_index, token.get_symbol_string(Exchange::Lighter
                                )
                            )));
                        }
                    } else {
                        info!("#{} | position not found in positions, which means it closed: {}", self.wallet.id, position.symbol);
                        info!("#{} | 🔴🔴 position closed: {}", self.wallet.id, position.symbol);
                        return Ok(());
                    }
                }
                Err(e) => {
                    return Err(TradingError::ExchangeError(format!(
                        "#{} | failed to confirm close of position {}: {}",
                        self.wallet.id, position.symbol, e
                    )));
                }
            }
        }
    
        unreachable!()
    }

    /// Determines if a position should be closed based on its value.
    ///
    /// # Arguments
    ///
    /// * `position` - Reference to the position to check
    ///
    /// # Returns
    ///
    /// * `bool` - `true` if position has non-zero value and should be closed
    fn should_close_position(&self, position: &LighterPosition) -> bool {
        let position_value = position.position_value
            .parse::<Decimal>()
            .unwrap_or(Decimal::ZERO);
        
        position_value > Decimal::ZERO
    }

    /// Parses position sign integer into PositionSide tuples.
    ///
    /// Converts the internal position sign representation (-1, 1) into
    /// readable PositionSide enums for both current and closing sides.
    ///
    /// # Arguments
    ///
    /// * `sign` - The position sign integer (-1 for short, 1 for long)
    ///
    /// # Returns
    ///
    /// * `Result<(PositionSide, PositionSide), TradingError>` - Tuple of (current_side, closing_side)
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if the sign value is invalid
    fn parse_position_sides(&self, sign: i32) -> Result<(PositionSide, PositionSide), TradingError> {
        match sign {
            1 => Ok((PositionSide::Long, PositionSide::Short)),
            -1 => Ok((PositionSide::Short, PositionSide::Long)),
            _ => Err(TradingError::InvalidInput(format!(
                "Invalid position sign: {}", sign
            ))),
        }
    }

    /// Converts a floating-point amount to base amount with proper decimal scaling.
    ///
    /// This method handles the conversion from human-readable decimal amounts
    /// to the integer-based representation required by the Lighter API.
    ///
    /// # Arguments
    ///
    /// * `amount` - The floating-point amount to convert
    ///
    /// # Returns
    ///
    /// * `Result<u64, TradingError>` - The scaled base amount as integer
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if conversion or parsing fails
    fn base_amount_from_f64(&self, amount: f64) -> Result<u64, TradingError> {
        // Convert to Decimal via string to preserve exact decimal representation
        let decimal_str = amount.to_string();
        let decimal = Decimal::from_str(&decimal_str).unwrap();
        
        // Get the number of decimal places
        let scale = decimal.scale();
        
        // Multiply by 10^scale to shift decimal point all the way right
        let multiplier = Decimal::from(10_u64.pow(scale));
        let result = decimal * multiplier;
        
        // Convert to u64
        result.round().to_string().parse::<u64>().map_err(|e| TradingError::InvalidInput(e.to_string()))
    }

    /// Calculates the base token amount for a given USDC investment at specified price.
    ///
    /// This method converts a USDC-denominated position size into the equivalent
    /// base token amount, accounting for token-specific denomination scaling.
    ///
    /// # Arguments
    ///
    /// * `token` - Reference to the token being traded
    /// * `amount_usdc` - The USDC amount to invest
    /// * `price` - The current market price (scaled by 10,000)
    ///
    /// # Returns
    ///
    /// * `Result<u64, TradingError>` - The calculated base token amount
    pub async fn calculate_base_amount(&self, token: &Token, amount_usdc: Decimal, price: u64) -> Result<u64, TradingError> {
        // Price is scaled by 10,000, so divide it
        let price_decimal = Decimal::from(price) / Decimal::from(10_000);
    
        // Calculate base amount
        let base_amount = amount_usdc / price_decimal;
        let base_amount_scaled = base_amount * token.get_denomination();    
        let base_rounded = base_amount_scaled.round().to_string().parse::<u64>().unwrap();
        Ok(base_rounded)
    }

    /// Updates the leverage for a specific token.
    ///
    /// This method sets the leverage and margin mode for a trading pair.
    /// Currently uses fixed leverage of 33.33x (3333 basis points).
    ///
    /// # Arguments
    ///
    /// * `token` - The token to update leverage for
    ///
    /// # Returns
    ///
    /// * `Result<(), TradingError>` - Success if leverage update is submitted
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Nonce retrieval fails
    /// * Transaction signing fails
    /// * API submission fails
    #[allow(unused)]
    pub async fn update_leverage(&self, token: Token) -> Result<(), TradingError> {
        let margin_mode = 0;
        let leverage_fraction = 3333;
        let nonce = self.get_nonce().await?;
        let market_index = token.get_market_index(Exchange::Lighter);

        let leverage_signed = self.signer_client.sign_update_leverage(
            market_index,
            leverage_fraction,
            margin_mode,
            nonce,
        )?;


        let tx_info_encoded = urlencoding::encode(&leverage_signed);
        let body = format!(
            "tx_type={}&tx_info={}&price_protection={}", 
            TX_TYPE_UPDATE_LEVERAGE, 
            tx_info_encoded, 
            false
        );

        let leverage_hash = self.send_tx(body).await?;
        info!("#{} | Leverage updated successfully! Hash: {}", self.wallet.id, leverage_hash);
        Ok(())
    }

    /// Executes a market order with retry logic for nonce errors.
    ///
    /// This method handles the complete order execution flow including:
    /// - Nonce management with automatic retry on invalid nonce errors
    /// - Order signing and submission
    /// - Price protection
    ///
    /// # Arguments
    ///
    /// * `token` - Reference to the token to trade
    /// * `side` - The position side (Long/Short)
    /// * `base_amount` - The amount of base token to trade
    /// * `price` - The limit price for the order
    /// * `close_position` - Whether this is a position-closing order
    ///
    /// # Returns
    ///
    /// * `Result<String, TradingError>` - The transaction hash of the executed order
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Nonce retrieval fails after retries
    /// * Order signing fails
    /// * API submission fails
    async fn execute_market_order(
        &self,
        token: &Token,
        side: PositionSide,
        base_amount: u64,
        price: u64,
        close_position: bool,
    ) -> Result<String, TradingError> {
        let market_index = token.get_market_index(Exchange::Lighter);
        let is_ask = matches!(side, PositionSide::Short);
        let reduce_only = matches!(close_position, true);
        let mut last_nonce_error: Option<String> = None;

        for attempt in 0..2 {
            let nonce = self.get_nonce().await?;

            info!(
                "#{} | Executing market {} order for {:?} with price {} | nonce: {} | attempt: {}",
                self.wallet.id, side, token, price, nonce, attempt
            );

            let order = LighterOrder::new(
                self.account_index,
                market_index,
                base_amount as i64,
                price as i64,
                is_ask,
                reduce_only,
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

            info!("#{} | order signed: {}", self.wallet.id, order_signed);
            let tx_info_encoded = urlencoding::encode(&order_signed);
            
            let body = format!(
                "tx_type={}&tx_info={}&price_protection={}",
                TX_TYPE_CREATE_ORDER,
                tx_info_encoded,
                false
            );

            let order_hash_result = self.send_tx(body).await;

            match order_hash_result {
                Ok(order_hash) => return Ok(order_hash),
                Err(e) => match e {
                    TradingError::InvalidNonce(e) => {
                        last_nonce_error = Some(e.clone());
                        warn!("#{} | Invalid nonce. Retrying...", self.wallet.id);

                        continue;
                    }
                    _ => return Err(e),
                },
            }
        }

        // If we reach here, all attempts failed due to nonce error
        if let Some(e) = last_nonce_error {
            return Err(TradingError::InvalidNonce(e));
        } else {
            return Err(TradingError::OrderExecutionFailed(
                "Failed to execute market order after multiple attempts".to_string(),
            ));
        }
    }
 
    /// Sends a signed transaction to the Lighter API.
    ///
    /// This low-level method handles the actual HTTP request to submit
    /// transactions to the Lighter backend.
    ///
    /// # Arguments
    ///
    /// * `body` - The URL-encoded transaction body
    ///
    /// # Returns
    ///
    /// * `Result<String, TradingError>` - The transaction hash
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * HTTP request fails
    /// * API returns non-200 response
    /// * Transaction hash not found in response
    async fn send_tx(&self, body: String) -> Result<String, TradingError> {
        let headers = self.get_headers();
        let url = format!("{}/sendTx", self.base_url);

        let response = Request::process_request(
            Method::POST, 
            url, 
            Some(headers), 
            Some(body), 
            self.wallet.proxy.clone()
        )
        .await;

        match response {
            Ok(response) => {
                match response["code"].as_i64() {
                    Some(code) => {
                        if code != 200 {
                            return Err(TradingError::InvalidInput(format!("Failed to send transaction: {}", response["message"].as_str().unwrap_or("Unknown error"))));
                        }
        
                        if let Some(tx_hash) = response.get("tx_hash") {
                            return Ok(tx_hash.to_string().replace("\"", ""));
                        }
        
                        return Err(TradingError::InvalidInput(format!("Tx hash not found in response: {:?}", response)));
                    },
                    None => return Err(TradingError::InvalidInput(format!("Code not found in response: {:?}", response))),
                }
            },
            Err(e) => {
                match e {
                    RequestError::ApiError(e) => {
                        if e.contains("invalid nonce") {
                            return Err(TradingError::InvalidNonce(e));
                        }

                        return Err(TradingError::OrderExecutionFailed(e.to_string()));
                    },
                    _ => {
                        return Err(TradingError::OrderExecutionFailed(e.to_string()));
                    }
                }
            }
        }
    }

    /// Creates the standard HTTP headers for Lighter API requests.
    ///
    /// These headers mimic a web browser request to avoid being blocked
    /// by API security measures.
    ///
    /// # Returns
    ///
    /// * `HeaderMap` - The configured HTTP headers
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


    /// Performs a health check by verifying authentication status.
    ///
    /// # Returns
    ///
    /// * `Result<bool, TradingError>` - `true` if healthy and authenticated
    async fn health_check(&self) -> Result<bool, TradingError> {
        Ok(self.is_authenticated().await)
    }


    /// Retrieves the balance for a specific asset.
    ///
    /// Currently only supports USDC balances on Lighter.
    ///
    /// # Arguments
    ///
    /// * `asset` - The asset symbol (currently only "USDC" is supported)
    ///
    /// # Returns
    ///
    /// * `Result<Balance, TradingError>` - The balance information
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if account retrieval or parsing fails
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

    /// Opens a new position with the specified parameters.
    ///
    /// This method implements the complete position opening flow:
    /// 1. Checks for existing positions (atomic operation requirement)
    /// 2. Calculates position size and gets current market price
    /// 3. Executes market order
    /// 4. Verifies position was successfully opened
    ///
    /// # Arguments
    ///
    /// * `token` - The token to trade
    /// * `side` - The position side (Long/Short)
    /// * `close_at` - The scheduled closing time for the position
    /// * `amount_usdc` - The USDC amount to invest in the position
    ///
    /// # Returns
    ///
    /// * `Result<Position, TradingError>` - The opened position details
    ///
    /// # Errors
    ///
    /// Returns `TradingError` if:
    /// * Existing positions are found (atomic operation violation)
    /// * Price retrieval fails
    /// * Order execution fails
    /// * Position verification fails
    async fn open_position(&self, token: Token, side: PositionSide, close_at: DateTime<Utc>, amount_usdc: Decimal) -> Result<Position, TradingError> {
        if let Ok(positions) = self.get_active_positions().await {
            if !positions.is_empty() {
                return Err(TradingError::AtomicOperationFailed("Position already open".to_string()));
            }
        }

        let price = self.get_market_price(&token, side).await?;
        let base_amount = self.calculate_base_amount(&token, amount_usdc, price).await?;
        let order_hash = self.execute_market_order(&token, side, base_amount, price, false).await?;
        info!("#{} | Order sent: {}", self.wallet.id, order_hash);        
        let tx = self.get_order_by_hash(&order_hash).await?;

        let positions = self.get_active_positions().await?;
        let market_index = token.get_market_index(Exchange::Lighter);
        if let Some(pos) = positions.iter().find(|p| p.market_id == market_index) {
            let id = uuid::Uuid::new_v4().to_string();
            let pos_value = pos.position_value.parse::<Decimal>().unwrap_or(Decimal::ZERO);

            if pos_value > Decimal::ZERO {
                info!("#{} | 🟢🟢 position opened: {}", self.wallet.id, tx.hash);

                return Ok(Position {
                    wallet_id: self.wallet.id,
                    id,
                    strategy_id: None,
                    exchange: Exchange::Lighter,
                    symbol: token.get_symbol_string(Exchange::Lighter),
                    side,
                    size: pos_value,
                    status: PositionStatus::Open,
                    opened_at: Utc::now(),
                    close_at,
                    closed_at: None,
                    realized_pnl: None,
                    updated_at: Utc::now(),
                });
            } else {
                return Err(TradingError::ExchangeError(format!(
                    "Open position on market index {} has zero size",
                    market_index
                )));
            }
        } else {
            return Err(TradingError::ExchangeError(format!(
                "It was not successful to open position on market index {} with token {}",
                market_index, 
                token.get_symbol_string(Exchange::Lighter)
            )));
        }
    }


    /// Retrieves the USDC balance for the account.
    ///
    /// # Returns
    ///
    /// * `Result<Decimal, TradingError>` - The available USDC balance
    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        let balance_usdc = self.get_balance("USDC").await?;
        Ok(balance_usdc.free)
    }

    /// Closes all open positions with market orders.
    ///
    /// This is a convenience method that delegates to the internal
    /// `close_all_positions` implementation.
    ///
    /// # Returns
    ///
    /// * `Result<(), TradingError>` - Success if all positions are closed
    async fn close_all_positions(&self) -> Result<(), TradingError> {
        self.close_all_positions().await
    }

    /// Closes a specific position.
    ///
    /// # Arguments
    ///
    /// * `position` - The position to close
    ///
    /// # Returns
    ///
    /// * `Result<Position, TradingError>` - The closed position details
    ///
    /// # Note
    ///
    /// This method is not yet fully implemented for Lighter exchange.
    async fn close_position(&self, position: &Position) -> Result<Position, TradingError> {
        todo!("Lighter close_position not fully implemented for {}", position.side);
    }
}
