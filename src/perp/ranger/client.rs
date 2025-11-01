use async_trait::async_trait;
use base64::Engine;
use chrono::{DateTime, Utc};
use http::{HeaderMap, Method};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use serde_json::json;
use solana_sdk::signer::Signer;
use crate::{error::TradingError, model::{balance::Balance, token::Token, Exchange, Position, PositionSide, PositionStatus}, perp::{ranger::models::{AuthRequest, AuthResponse, MarketOrderRequest, MarketOrderResponse}, PerpExchange}, request::Request, trader::wallet::Wallet};


/// Base URLs for Ranger Finance API endpoints
const BASE_URL_DATA: &str = "https://sor-437363704888.asia-northeast1.run.app";
const BASE_URL_TRADE: &str = "https://www.app.ranger.finance/api/hyperliquid";
const PRIVY_AUTH_URL: &str = "https://auth.privy.io/api/v1/siws/authenticate";
const PRIVY_APP_ID: &str = "cmeiaw35f00dzjl0bztzhen22";
const API_ACCESS_TOKEN: &str = "www.app.ranger.finance-1761716458384__d05d82036014beb4ed77cb628db90c7b119a0e346d7244837caea723733deff3";


/// Client for interacting with Ranger Finance perpetual exchange
/// 
/// This client handles authentication, order placement, and position management
/// through the Ranger Finance API using Solana wallet authentication.
#[allow(unused)]
pub struct RangerClient {
    wallet: Wallet,
    base_url_data: String,
    base_url_trade: String,
    auth_token: Option<String>,
}


impl RangerClient {
    /// Creates a new RangerClient instance with the provided wallet
    /// 
    /// # Arguments
    /// 
    /// * `wallet` - The wallet used for authentication and signing transactions
    /// 
    /// # Returns
    /// 
    /// A new RangerClient instance with default API endpoints
    pub fn new(wallet: &Wallet) -> Self {
        let base_url_data = BASE_URL_DATA.to_string();
        let base_url_trade = BASE_URL_TRADE.to_string();

        Self { wallet: wallet.clone(), base_url_data, base_url_trade, auth_token: None }
    }


    /// Authenticates with Ranger Finance using SIWS (Sign-In with Solana) protocol
    /// 
    /// This method:
    /// 1. Generates a nonce from the Privy authentication service
    /// 2. Creates a SIWS message with the nonce and current timestamp
    /// 3. Signs the message with the Solana wallet
    /// 4. Sends the signed message to Privy for authentication
    /// 5. Stores the received authentication token for future API calls
    /// 
    /// # Returns
    /// 
    /// * `Ok(String)` containing the authentication token on success
    /// * `Err(TradingError)` if any step in the authentication process fails
    /// 
    /// # Errors
    /// 
    /// Returns `TradingError` for:
    /// - Wallet operations failures
    /// - HTTP request failures
    /// - Authentication failures from the API
    /// - JSON parsing errors
    pub async fn set_access_token(&mut self) -> Result<String, TradingError> {
        let pubkey = self.wallet.get_solana_keypair()?.pubkey().to_string();
        
        let nonce = self.generate_nonce().await?;
        let issued_at = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        
        // Create SIWS message
        let message = format!(
            "www.app.ranger.finance wants you to sign in with your Solana account:\n{}\n\nYou are proving you own {}.\n\nURI: https://www.app.ranger.finance\nVersion: 1\nChain ID: mainnet\nNonce: {}\nIssued At: {}\nResources:\n- https://privy.io",
            pubkey, pubkey, nonce, issued_at
        );

        // Sign the message
        let signature = self.wallet.sign_solana_message(message.as_bytes())
            .map_err(|e| TradingError::AuthenticationFailed(e.to_string()))?;
        let signature_base64 = base64::engine::general_purpose::STANDARD.encode(&signature);

        info!("#{} | Signature: {}", self.wallet.id, signature_base64);
        let auth_request = AuthRequest {
            message,
            signature: signature_base64,
            wallet_client_type: "phantom".to_string(),
            connector_type: "solana_adapter".to_string(),
            mode: "login-or-sign-up".to_string(),
            message_type: "plain".to_string(),
        };

        let client = reqwest::Client::new();
        let response = client
            .post(PRIVY_AUTH_URL)
            .headers(self.build_auth_headers())
            .json(&auth_request)
            .send()
            .await
            .map_err(|e| TradingError::HttpError(e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TradingError::AuthenticationFailed(error_text));
        }

        let auth_response: AuthResponse = response
            .json()
            .await
            .map_err(|e| TradingError::HttpError(e))?;

        // Set the auth token
        self.auth_token = Some(auth_response.token.clone());        
        info!("#{} | Authentication successful. User ID: {}", self.wallet.id, auth_response.user.id);

        Ok(auth_response.token)
    }



    /// Generates a nonce for SIWS authentication by calling the Privy init endpoint
    /// 
    /// The nonce is a unique cryptographic value that prevents replay attacks
    /// and is required for the SIWS authentication flow.
    /// 
    /// # Returns
    /// 
    /// * `Ok(String)` containing the nonce received from the API
    /// * `Err(TradingError)` if the HTTP request fails or the response is invalid
    /// 
    /// # Errors
    /// 
    /// Returns `TradingError` for:
    /// - HTTP request failures
    /// - Non-200 HTTP responses
    /// - Missing nonce in response
    /// - JSON parsing errors
    async fn generate_nonce(&self) -> Result<String, TradingError> {
        let client = reqwest::Client::new();
        let headers = self.build_nonce_headers();

        info!("#{} | Generating nonce...", self.wallet.id);
        let response = client.post("https://auth.privy.io/api/v1/siws/init")
            .headers(headers)
            .json(&json!({
                "address": self.wallet.get_solana_keypair()?.pubkey().to_string()
            }))
            .send()
            .await
            .map_err(|e| TradingError::HttpError(e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(TradingError::AuthenticationFailed(format!(
                "Failed to get nonce: HTTP {}: {}",
                status, error_text
            )));
        }

        let init_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| TradingError::HttpError(e))?;

        let nonce = init_response["nonce"]
            .as_str()
            .ok_or_else(|| TradingError::AuthenticationFailed("No nonce in response".to_string()))?
            .to_string();

        info!("#{} | Got nonce: {}", self.wallet.id, nonce);
        Ok(nonce)
    }



    /// Opens a new position on Ranger Finance
    /// 
    /// This method executes a market order to open a long or short position
    /// for the specified token with the given size and collateral.
    /// 
    /// # Arguments
    /// 
    /// * `token` - The token to trade (e.g., RENDER, SOL, etc.)
    /// * `side` - The position side (Long or Short)
    /// * `base_amount` - The amount of the base token to trade
    /// * `amount_usdc` - The amount of USDC to use as collateral
    /// 
    /// # Returns
    /// 
    /// * `Ok(MarketOrderResponse)` containing the order execution details
    /// * `Err(TradingError)` if the order placement fails
    /// 
    /// # Errors
    /// 
    /// Returns `TradingError` for:
    /// - Authentication failures
    /// - HTTP request failures
    /// - Order execution failures from the API
    /// - JSON parsing errors
    pub async fn open_position(
        &self, 
        token: &Token, 
        side: PositionSide,
        close_at: DateTime<Utc>,
        amount_usdc: f64, 
    ) -> Result<Position, TradingError> {
        let price = self.get_token_price(token).await?;
        info!("#{} | Token: {}, Price: {}", self.wallet.id, token.symbol, price);
        let base_amount = self.build_base_amount(amount_usdc, price)?;
        info!("#{} | Base amount: {}", self.wallet.id, base_amount);

        let headers = self.build_trade_headers()?;
        let url = format!("{}/increase_position", self.base_url_trade);
        let payload: MarketOrderResponse = self.build_open_position_payload(token, side, base_amount, amount_usdc).await?;
        
        println!("Headers: {:?}", headers);
        println!("Payload: {:?}", payload.hyperliquid_payload);

        let response = Request::process_request(
            Method::POST,
            url,
            Some(headers),
            Some(json!({"order": payload.hyperliquid_payload}).to_string()),
            self.wallet.proxy.clone()
        ).await;

        match response {
            Ok(response) => {
                let response_json = response.get("message").and_then(|message| message.as_str()).unwrap_or_default();
                if response_json.contains("Successfully increased position!") {
                    let order_id = response.get("oid").and_then(|order_id| order_id.as_i64()).unwrap_or_default();
                    
                    return Ok(Position {
                        wallet_id: self.wallet.id,
                        id: order_id.to_string(),
                        strategy_id: None,
                        exchange: Exchange::Ranger,
                        symbol: token.get_symbol_string(Exchange::Ranger),
                        side,
                        size: Decimal::from_f64(base_amount).unwrap(),
                        status: PositionStatus::Open,
                        opened_at: Utc::now(),
                        close_at,
                        closed_at: None,
                        realized_pnl: None,
                        updated_at: Utc::now(),
                    });
                } else {
                    return Err(TradingError::OrderExecutionFailed(response_json.to_string()));
                }
            }
            Err(e) => {
                return Err(TradingError::OrderExecutionFailed(e.to_string()));
            }
        }
    }

    /// Fetches the current USD price for a given token by querying the external spot pricing API.
    ///
    /// # Arguments
    ///
    /// * `token` - The token for which to fetch the price.
    ///
    /// # Returns
    ///
    /// * `Ok(Decimal)` containing the price if retrieval is successful.
    /// * `Err(TradingError)` if the price cannot be fetched or parsed.
    ///
    /// # Errors
    ///
    /// Returns `TradingError::MarketDataUnavailable` for API failures, non-200 responses, or if the
    /// price information is missing or incorrectly formatted in the API response.
    async fn get_token_price(&self, token: &Token) -> Result<f64, TradingError> {
        let client = reqwest::Client::new();
        let headers = self.build_data_headers();
        let address = token.get_address()?;
        let url = format!("https://prod-spot-pricing-api-437363704888.asia-northeast1.run.app/defi/multi_price?list_address={}", address);

        let response = client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| TradingError::HttpError(e))?;

        if !response.status().is_success() {
            return Err(TradingError::MarketDataUnavailable(format!(
                "Failed to get token price: HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let response_json = response.json::<serde_json::Value>().await.map_err(|e| TradingError::HttpError(e))?;

        // Traverse: response_json["data"][address]["value"]
        let price_value = response_json
            .get("data")
            .and_then(|data| data.get(&address))
            .and_then(|addr_obj| addr_obj.get("value"));

        let price = match price_value {
            Some(val) => {
                // Accept either string or number values
                if let Some(decimal_num) = val.as_f64() {
                    decimal_num
                } else if let Some(s) = val.as_str() {
                    s.parse::<f64>().map_err(|e| TradingError::MarketDataUnavailable(format!(
                        "Failed to parse price string to float for address {}: value = {}, err = {}", address, s, e
                    )))?
                } else {
                    return Err(TradingError::MarketDataUnavailable(format!(
                        "Price for address {} not a valid string or float: {:?}", address, val
                    )));
                }
            }
            None => {
                return Err(TradingError::MarketDataUnavailable(format!(
                    "No price found in response for address {}", address
                )));
            }
        };

        Ok(price)
    }

    /// Calculates the base token amount from a given USDC amount and token price.
    ///
    /// This method converts a USDC-denominated position size into the equivalent
    /// base token amount using the specified price. The result is a Decimal value.
    ///
    /// # Arguments
    ///
    /// * `amount_usdc` - The USDC amount to invest as a Decimal.
    /// * `price` - The price per base token as a Decimal.
    ///
    /// # Returns
    ///
    /// * `Result<f64, TradingError>` - The calculated base token amount or an error.
    fn build_base_amount(&self, amount_usdc: f64, price: f64) -> Result<f64, TradingError> {
        let base_amount = amount_usdc / price;
        Ok(base_amount)
    }

    /// Builds the payload for opening a position by calling the data service
    /// 
    /// This method prepares the market order request with the necessary parameters
    /// and retrieves the formatted payload from the data service that can be
    /// used to execute the actual trade.
    /// 
    /// # Arguments
    /// 
    /// * `token` - The token to trade
    /// * `side` - The position side (Long or Short)
    /// * `base_amount` - The amount of the base token to trade
    /// * `amount_usdc` - The amount of USDC to use as collateral
    /// 
    /// # Returns
    /// 
    /// * `Ok(MarketOrderResponse)` containing the formatted order payload
    /// * `Err(TradingError)` if the payload generation fails
    /// 
    /// # Errors
    /// 
    /// Returns `TradingError` for:
    /// - HTTP request failures to the data service
    /// - Non-200 HTTP responses
    /// - JSON parsing errors
    async fn build_open_position_payload(
        &self, 
        token: &Token, 
        side: PositionSide, 
        base_amount: f64, 
        amount_usdc: f64, 
    ) -> Result<MarketOrderResponse, TradingError> {

        let client = reqwest::Client::new();
        let headers = self.build_data_headers();
        let symbol = token.get_symbol_string(Exchange::Ranger);
        let url = format!("{}/increase_position", self.base_url_data);

        let evm_address = "0x90C7f18f99f931fa449A754252af56296Cb03ba1".to_string();
        let fee_payer = self.wallet.get_solana_keypair()?.pubkey().to_string();

        let side_str = match side {
            PositionSide::Long => "Long",
            PositionSide::Short => "Short",
        }.to_string();

        let request = MarketOrderRequest {
            fee_payer: fee_payer.clone(),
            symbol: symbol.to_uppercase(),
            side: side_str,
            adjustment_type: "Increase".to_string(),
            size: base_amount,
            collateral: amount_usdc,
            size_denomination: symbol.to_uppercase(),
            collateral_denomination: "USDC".to_string(),
            evm_address: evm_address.clone(),
            ..Default::default()
        };

        let response = client
            .post(url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| TradingError::HttpError(e))?;


        if !response.status().is_success() {
            return Err(TradingError::OrderExecutionFailed(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let response = response.json::<MarketOrderResponse>().await.map_err(|e| TradingError::HttpError(e))?;
        Ok(response)
    }


    /// Builds headers for authentication requests to Privy
    /// 
    /// These headers include the necessary authentication metadata,
    /// security headers, and platform information required by the
    /// Privy authentication service.
    /// 
    /// # Returns
    /// 
    /// HeaderMap configured for Privy authentication requests
    fn build_auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("accept", "application/json".parse().unwrap());
        headers.insert("accept-language", "en-US,en;q=0.9,ru;q=0.8,uk;q=0.7".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("origin", "https://www.app.ranger.finance".parse().unwrap());
        headers.insert("priority", "u=1, i".parse().unwrap());
        headers.insert("privy-app-id", PRIVY_APP_ID.parse().unwrap());
        headers.insert("privy-ca-id", "13f602fd-7df7-490a-b164-7ea11e2ac973".parse().unwrap());
        headers.insert("privy-client", "react-auth:2.21.1".parse().unwrap());
        headers.insert("referer", "https://www.app.ranger.finance/".parse().unwrap());
        headers.insert("sec-ch-ua", "\"Google Chrome\";v=\"141\", \"Not?A_Brand\";v=\"8\", \"Chromium\";v=\"141\"".parse().unwrap());
        headers.insert("sec-ch-ua-mobile", "?0".parse().unwrap());
        headers.insert("sec-ch-ua-platform", "\"macOS\"".parse().unwrap());
        headers.insert("sec-fetch-dest", "empty".parse().unwrap());
        headers.insert("sec-fetch-mode", "cors".parse().unwrap());
        headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
        headers.insert("sec-fetch-storage-access", "active".parse().unwrap());
        headers.insert("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36".parse().unwrap());
    
        headers
    }



    /// Builds headers for data service API requests
    /// 
    /// These headers are used for requests to the data processing service
    /// that prepares order payloads and market data.
    /// 
    /// # Returns
    /// 
    /// HeaderMap configured for data service requests
    fn build_data_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
    
        headers.insert("accept", "*/*".parse().unwrap());
        headers.insert("accept-language", "en-US,en;q=0.9,ru;q=0.8,uk;q=0.7".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("origin", "https://www.app.ranger.finance".parse().unwrap());
        headers.insert("referer", "https://www.app.ranger.finance/".parse().unwrap());
        headers.insert("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36".parse().unwrap());

        headers
    }


    /// Builds headers for nonce generation requests
    /// 
    /// These headers are specifically for the initial nonce request
    /// to the Privy SIWS init endpoint.
    /// 
    /// # Returns
    /// 
    /// HeaderMap configured for nonce generation requests
    fn build_nonce_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        headers.insert("accept", "application/json".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("privy-app-id", PRIVY_APP_ID.parse().unwrap());
        headers.insert("privy-client", "react-auth:2.21.1".parse().unwrap());
        headers.insert("origin", "https://www.app.ranger.finance".parse().unwrap());
        headers.insert("referer", "https://www.app.ranger.finance/".parse().unwrap());
        headers.insert("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36".parse().unwrap());

        headers
    }


    /// Builds headers for trade execution requests
    /// 
    /// These headers include the authentication token and all required cookies,
    /// which are used for actual trade execution requests to the trading API.
    /// 
    /// # Returns
    /// 
    /// * `Ok(HeaderMap)` configured for trade execution with authentication
    /// * `Err(TradingError)` if the authentication token is not available
    /// 
    /// # Errors
    /// 
    /// Returns `TradingError::AuthenticationFailed` if no auth token is set
    fn build_trade_headers(&self) -> Result<HeaderMap, TradingError> {
        let mut headers = HeaderMap::new();
        headers.insert("accept", "*/*".parse().unwrap());
        headers.insert(
            "accept-language",
            "en-US,en;q=0.9,ru;q=0.8,uk;q=0.7".parse().unwrap(),
        );
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert(
            "origin",
            "https://www.app.ranger.finance".parse().unwrap(),
        );
        headers.insert(
            "referer",
            "https://www.app.ranger.finance/perps".parse().unwrap(),
        );
        headers.insert("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36".parse().unwrap());

        // Add Authorization header (Privy token)
        if let Some(token) = &self.auth_token {
            let auth_value = format!("Bearer {}", token);
            headers.insert(
                "authorization",
                auth_value
                    .parse()
                    .map_err(|_| TradingError::AuthenticationFailed(format!("Can't parse Auth Token from string!")))?,
            );
        } else {
            return Err(TradingError::AuthenticationFailed(format!("Auth Token is not set!")));
        }

        // Build cookie string exactly as in the curl example
        let mut cookies = Vec::new();
        
        // Add _ga cookie if available
        cookies.push(format!("_ga={}", "GA1.2.1719891219.1"));
        
        // Add privy-session
        cookies.push("privy-session=t".to_string());


        // Add api access tokens
        cookies.push(format!("serverless-api-access-token={}", API_ACCESS_TOKEN));
        

        // Add privy-token (same as auth token)
        if let Some(token) = &self.auth_token {
            cookies.push(format!("privy-token={}", token));
        }
        
        // Join all cookies with "; "
        let cookie_string = cookies.join("; ");
        headers.insert(
            "cookie",
            cookie_string
                .parse()
                .map_err(|_| TradingError::AuthenticationFailed(format!("Can't parse cookies from string!")))?,
        );

        Ok(headers)
    }
}

#[async_trait]
impl PerpExchange for RangerClient {
    fn name(&self) -> &str {
        "Ranger Finance"
    }

    /// Checks the health of the Ranger Finance exchange client
    /// 
    /// Verifies that the client can successfully authenticate and
    /// communicate with the exchange APIs.
    /// 
    /// # Returns
    /// 
    /// * `Ok(true)` if the client is healthy and operational
    /// * `Ok(false)` if the client has issues but can recover
    /// * `Err(TradingError)` if the health check fails completely
    async fn health_check(&self) -> Result<bool, TradingError> {
        todo!()
    }

    #[allow(unused)]
    async fn get_balance(&self, asset: &str) -> Result<Balance, TradingError> {
        todo!()
    }

    #[allow(unused)]
    async fn open_position(&self, token: Token, side: PositionSide, close_at: DateTime<Utc>, amount_usdc: Decimal) -> Result<Position, TradingError> {
        todo!()
    }

    #[allow(unused)]
    async fn close_position(&self, position: &Position) -> Result<Position, TradingError> {
        todo!()
    }

    #[allow(unused)]
    async fn close_all_positions(&self) -> Result<(), TradingError> {
        todo!()
    }

    #[allow(unused)]
    async fn get_usdc_balance(&self) -> Result<Decimal, TradingError> {
        todo!()
    }
}