use std::time::Duration;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{header::HeaderMap, Method};
use rust_decimal::Decimal;
use tokio::time::sleep;
use urlencoding;
use alloy::signers::{Signature, SignerSync};
use alloy::signers::local::PrivateKeySigner;
use alloy::primitives::eip191_hash_message;
use crate::model::PositionStatus;
use crate::perp::lighter::models::{LighterPosition, LighterTx};
use crate::{error::TradingError, model::{balance::Balance, token::Token, Exchange, Position, PositionSide}, perp::{lighter::{models::{LighterAccount, LighterOrder}, signer::SignerClient}, PerpExchange}, request::Request, trader::wallet::Wallet};
use crate::perp::lighter::signer::{TX_TYPE_CHANGE_PUB_KEY, TX_TYPE_CREATE_ORDER, TX_TYPE_UPDATE_LEVERAGE};

const DEFAULT_API_KEY_INDEX: i32 = 0;
const DEFAULT_BASE_URL: &str = "https://mainnet.zklighter.elliot.ai/api/v1";
const MIN_TRADING_BALANCE: i64 = 10;

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


    #[allow(unused)]
    pub async fn is_authenticated(&self) -> bool {
        self.get_account().await.is_ok()
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
        
        Ok((api_private_key, api_public_key))
    }

    async fn get_account(&self) -> Result<LighterAccount, TradingError> {
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

    async fn get_market_price(&self, token: &Token, side: PositionSide) -> Result<u64, TradingError> {
        let end_timestamp = Utc::now().timestamp_millis();
        let start_timestamp = end_timestamp - 60000;
        let url = format!("{}/candlesticks?market_id={}&resolution=1m&start_timestamp={}&end_timestamp={}&count_back=5", self.base_url, token.get_market_index(Exchange::Lighter), start_timestamp, end_timestamp);
        let response = Request::process_request(Method::GET, url, None, None, None).await?;

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

                let adjusted_price_f64 = match side {
                    PositionSide::Short => price_f64 * 0.995,
                    PositionSide::Long => price_f64 * 1.005,
                };

                let price = (adjusted_price_f64 * 100.0) as u64;
                Ok(price)
            },
            None => return Err(TradingError::InvalidInput(format!("Candlesticks not found in response: {:?}", response))),
        }
    }

    async fn get_order_by_hash(&self, hash: &str) -> Result<LighterTx, TradingError> {
        let url = format!("{}/tx?by=hash&value={}", self.base_url, hash);
        let mut last_err = None;

        for attempt in 1..=10 {
            let response = match Request::process_request(Method::GET, url.clone(), None, None, None).await {
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
                    if tx.code == 200 && tx.status == 3 {
                        return Ok(tx);
                    } else {
                        last_err = Some(
                            TradingError::OrderExecutionFailed(
                                format!("Attempt {attempt}: Failed to get order by hash: {:?}", response)
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

    async fn close_all_positions(&self) -> Result<(), TradingError> {
        let positions = self.get_active_positions().await?;

        println!("Positions: {:#?}", positions);
        for position in positions {
            if position.position_value.parse::<Decimal>().unwrap_or(Decimal::ZERO) > Decimal::ZERO {
                let token_id = position.market_id;
                let token = Token::from_market_index(Exchange::Lighter, token_id);

                let position_side = match position.sign {
                    1 => PositionSide::Long,
                    -1 => PositionSide::Short,
                    _ => return Err(TradingError::InvalidInput(format!("Invalid position sign: {}", position.sign))),
                };

                let position_side_to_close: PositionSide = match position.sign {
                    1 => PositionSide::Short,
                    -1 => PositionSide::Long,
                    _ => return Err(TradingError::InvalidInput(format!("Invalid position sign: {}", position.sign))),
                };

                let price = self.get_market_price(&token, position_side_to_close).await?;
                info!("#{} | Found open {} position to close: {}", self.wallet.id, position_side, position.symbol);

                let base_amount = if position_side == PositionSide::Long {
                    0
                } else {
                    self.calculate_base_amount(
                        position.position_value.parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO), price)
                        .await?
                };


                let order = self.execute_market_order(&token, position_side_to_close, base_amount, price, true).await?;
                self.get_order_by_hash(&order).await?;

                info!("#{} | 🔴🔴 Position closed: {} | PnL: {} USDC", self.wallet.id, position.symbol, position.realized_pnl);
            }
        }

        Ok(())
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

    async fn execute_market_order(
        &self,
        token: &Token,
        side: PositionSide,
        base_amount: u64,
        price: u64,
        close_position: bool,
    ) -> Result<String, TradingError> {
        let nonce = self.get_nonce().await?;
        let market_index = token.get_market_index(Exchange::Lighter);

        let is_ask = match side {
            PositionSide::Long => false,
            PositionSide::Short => true,
        };

        let reduce_only = match side {
            PositionSide::Long => false,
            PositionSide::Short => if close_position { true } else { false },
        };

        info!("#{} | Executing market {} order for {:?} with price {} | nonce: {}", self.wallet.id, side, token, price, nonce);

        let order = LighterOrder::new(
            self.account_index,
            market_index,
            base_amount as i64,
            price as i64,
            is_ask, 
            reduce_only,
            nonce,
        );

        println!("Order: {:?}", order);
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

        let tx_info_encoded = urlencoding::encode(&order_signed);
        
        let body = format!(
            "tx_type={}&tx_info={}&price_protection={}",
            TX_TYPE_CREATE_ORDER,
            tx_info_encoded,
            false
        );

        let order_hash = self.send_tx(body).await?;
        Ok(order_hash)
    }
 
    async fn send_tx(&self, body: String) -> Result<String, TradingError> {
        let headers = self.get_headers();
        let url = format!("{}/sendTx", self.base_url);

        let response = Request::process_request(
            Method::POST, 
            url, 
            Some(headers), 
            Some(body), 
            None
        )
        .await?;


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
        };

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


    async fn open_position(&self, token: Token, side: PositionSide, close_at: DateTime<Utc>, _amount_usdc: Decimal) -> Result<Position, TradingError> {
        if let Ok(positions) = self.get_active_positions().await {
            if !positions.is_empty() {
                return Err(TradingError::AtomicOperationFailed("Position already open".to_string()));
            }
        }

        let balance_usdc = self.get_usdc_balance().await?;
        let price = self.get_market_price(&token, side).await?;
        let base_amount = self.calculate_base_amount(balance_usdc, price).await?;

        if balance_usdc < Decimal::from(MIN_TRADING_BALANCE) {
            return Err(TradingError::InsufficientBalance(
                format!("Insufficient balance for USDC! Balance: {:.2} USDC", balance_usdc))
            );
        }

        let order_hash = self.execute_market_order(&token, side, base_amount, price, false).await?;
        info!("#{} | Order sent: {}", self.wallet.id, order_hash);
        
        let tx = self.get_order_by_hash(&order_hash).await?;
        info!("#{} | 🟢🟢 Order executed: {}", self.wallet.id, tx.hash);

        let positions = self.get_active_positions().await?;
        let market_index = token.get_market_index(Exchange::Lighter);
        if let Some(pos) = positions.iter().find(|p| p.market_id == market_index) {
            let id = uuid::Uuid::new_v4().to_string();
            let pos_value = pos.position_value.parse::<Decimal>().unwrap_or(Decimal::ZERO);

            if pos_value > Decimal::ZERO {
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


    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        let balance_usdc = self.get_balance("USDC").await?;
        Ok(balance_usdc.free)
    }

    async fn close_all_positions(&self) -> Result<(), TradingError> {
        self.close_all_positions().await
    }

    async fn close_position(&self, position: &Position) -> Result<Position, TradingError> {
        todo!("Lighter close_position not fully implemented for {}", position.side);
    }
}

